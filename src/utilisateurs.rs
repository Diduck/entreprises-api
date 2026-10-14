//! Comptes utilisateurs : mots de passe hachés en Argon2id, création et lecture en base, validation des corps JSON.

use std::sync::LazyLock;

use argon2::{
    Argon2,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};
use serde::Serialize;
use serde_json::{Map, Value};
use sqlx::MySqlPool;
use utoipa::ToSchema;
use validator::ValidateEmail;

use crate::{
    auth::Role,
    errors::{ApiError, FieldError},
};

pub const MOT_DE_PASSE_MIN: usize = 12;
pub const MOT_DE_PASSE_MAX: usize = 128;
const EMAIL_MAX: usize = 254;

// ---------------------------------------------------------------------------
// Mots de passe
// ---------------------------------------------------------------------------

/// Empreinte Argon2id (paramètres par défaut, sel aléatoire) au format PHC : `$argon2id$v=19$…`.
///
/// Volontairement lent (~quelques dizaines de ms) : à appeler via `tokio::task::spawn_blocking`.
pub fn hacher(mot_de_passe: &str) -> String {
    Argon2::default().hash_password(mot_de_passe.as_bytes()).expect("hachage Argon2 avec sel aléatoire").to_string()
}

/// Compare un mot de passe à une empreinte stockée. Lent, comme `hacher`.
pub fn verifier(mot_de_passe: &str, empreinte: &str) -> bool {
    PasswordHash::new(empreinte)
        .is_ok_and(|empreinte| Argon2::default().verify_password(mot_de_passe.as_bytes(), &empreinte).is_ok())
}

/// Empreinte de référence pour les emails inconnus : la connexion prend alors le même temps que pour un
/// email existant, ce qui empêche de deviner quels comptes existent en chronométrant les réponses.
static EMPREINTE_FACTICE: LazyLock<String> = LazyLock::new(|| hacher("mot-de-passe-factice-jamais-valide"));

/// Calcule l'empreinte de référence au démarrage, pour que la première connexion ne soit pas plus lente.
pub fn prechauffer() {
    LazyLock::force(&EMPREINTE_FACTICE);
}

/// Vérifie email + mot de passe ; `None` si l'un des deux est faux (sans dire lequel).
pub async fn authentifier(pool: &MySqlPool, email: &str, mot_de_passe: String) -> Result<Option<Compte>, ApiError> {
    let compte = par_email(pool, email).await?;
    let empreinte = compte.as_ref().map_or_else(|| EMPREINTE_FACTICE.clone(), |c| c.mot_de_passe.clone());
    let valide = tokio::task::spawn_blocking(move || verifier(&mot_de_passe, &empreinte))
        .await
        .expect("vérification du mot de passe");
    Ok(compte.filter(|_| valide))
}

// ---------------------------------------------------------------------------
// Base de données
// ---------------------------------------------------------------------------

/// Ligne de la table `utilisateurs`, empreinte comprise (jamais renvoyée par l'API).
#[derive(Debug, sqlx::FromRow)]
pub struct Compte {
    pub id: i64,
    pub email: String,
    pub mot_de_passe: String,
    pub role: String,
    pub cree_le: String,
}

impl Compte {
    pub fn role(&self) -> Role {
        Role::parse(&self.role).expect("rôle garanti par la contrainte CHECK")
    }
}

/// Représentation JSON d'un utilisateur : sans l'empreinte du mot de passe.
#[derive(Debug, Serialize, ToSchema)]
pub struct Utilisateur {
    #[schema(example = 3)]
    pub id: i64,
    #[schema(example = "alice@exemple.fr")]
    pub email: String,
    #[schema(value_type = Role)]
    pub role: String,
    #[schema(example = "2026-09-23T14:05:00Z", format = DateTime)]
    pub cree_le: String,
    pub links: Liens,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Liens {
    #[serde(rename = "self")]
    #[schema(example = "/api/v1/utilisateurs/3")]
    pub self_: String,
}

impl From<Compte> for Utilisateur {
    fn from(compte: Compte) -> Self {
        Self {
            links: Liens { self_: chemin(compte.id) },
            id: compte.id,
            email: compte.email,
            role: compte.role,
            cree_le: compte.cree_le,
        }
    }
}

pub fn chemin(id: i64) -> String {
    format!("/api/v1/utilisateurs/{id}")
}

macro_rules! select_utilisateurs {
    ($filtre:literal) => {
        concat!("SELECT id, email, mot_de_passe, role, cree_le FROM utilisateurs WHERE ", $filtre)
    };
}

pub async fn par_email(pool: &MySqlPool, email: &str) -> sqlx::Result<Option<Compte>> {
    sqlx::query_as(select_utilisateurs!("email = ?")).bind(email).fetch_optional(pool).await
}

pub async fn par_id(pool: &MySqlPool, id: i64) -> sqlx::Result<Option<Compte>> {
    sqlx::query_as(select_utilisateurs!("id = ?")).bind(id).fetch_optional(pool).await
}

/// Crée le compte ; `None` si l'email est déjà pris.
pub async fn creer(pool: &MySqlPool, nouveau: &NouvelUtilisateur, empreinte: &str) -> sqlx::Result<Option<Compte>> {
    let resultat = sqlx::query(
        "INSERT INTO utilisateurs (email, mot_de_passe, role, cree_le) \
         VALUES (?, ?, ?, DATE_FORMAT(UTC_TIMESTAMP(), '%Y-%m-%dT%H:%i:%sZ'))",
    )
    .bind(&nouveau.email)
    .bind(empreinte)
    .bind(nouveau.role.as_str())
    .execute(pool)
    .await;
    match resultat {
        Ok(insertion) => par_id(pool, insertion.last_insert_id() as i64).await,
        Err(sqlx::Error::Database(erreur)) if erreur.is_unique_violation() => Ok(None),
        Err(erreur) => Err(erreur),
    }
}

// ---------------------------------------------------------------------------
// Corps JSON
// ---------------------------------------------------------------------------

/// Corps de `POST /api/v1/tokens`.
#[derive(Debug, ToSchema)]
pub struct Connexion {
    #[schema(example = "lecteur@demo.fr")]
    pub email: String,
    #[schema(example = "lecteur-demo-mdp", format = Password)]
    pub mot_de_passe: String,
}

impl Connexion {
    pub fn depuis_json(corps: Value) -> Result<Self, ApiError> {
        let mut objet = objet(corps, r#"{ "email": "…", "mot_de_passe": "…" }"#)?;
        let mut erreurs = Vec::new();
        let email = texte_obligatoire(&mut objet, "email", &mut erreurs);
        let mot_de_passe = texte_obligatoire(&mut objet, "mot_de_passe", &mut erreurs);
        champs_inconnus(&objet, &mut erreurs);
        match (email, mot_de_passe) {
            (Some(email), Some(mot_de_passe)) if erreurs.is_empty() => {
                Ok(Self { email: normaliser_email(&email), mot_de_passe })
            }
            _ => Err(ApiError::Validation(erreurs)),
        }
    }
}

/// Corps de `POST /api/v1/utilisateurs`, validé.
#[derive(Debug, ToSchema)]
pub struct NouvelUtilisateur {
    #[schema(example = "alice@exemple.fr", format = Email, max_length = 254)]
    pub email: String,
    #[schema(example = "un-mot-de-passe-solide", format = Password, min_length = 12, max_length = 128)]
    pub mot_de_passe: String,
    pub role: Role,
}

impl NouvelUtilisateur {
    /// Lit et valide le corps ; renvoie tous les champs refusés d'un coup (422).
    pub fn depuis_json(corps: Value) -> Result<Self, ApiError> {
        let mut objet = objet(corps, r#"{ "email": "…", "mot_de_passe": "…", "role": "lecteur" }"#)?;
        let mut erreurs = Vec::new();
        let email = texte_obligatoire(&mut objet, "email", &mut erreurs);
        let mot_de_passe = texte_obligatoire(&mut objet, "mot_de_passe", &mut erreurs);
        let role = texte_obligatoire(&mut objet, "role", &mut erreurs);
        champs_inconnus(&objet, &mut erreurs);
        let (Some(email), Some(mot_de_passe), Some(role)) = (email, mot_de_passe, role) else {
            return Err(ApiError::Validation(erreurs));
        };
        Self::valider(&email, mot_de_passe, &role, erreurs)
    }

    /// Validation commune à l'API et à la commande `creer-utilisateur`.
    pub fn valider(
        email: &str,
        mot_de_passe: String,
        role: &str,
        mut erreurs: Vec<FieldError>,
    ) -> Result<Self, ApiError> {
        let email = normaliser_email(email);
        if email.chars().count() > EMAIL_MAX || !email.validate_email() {
            erreurs.push(FieldError::new("email", "Adresse email invalide."));
        }
        let longueur = mot_de_passe.chars().count();
        if !(MOT_DE_PASSE_MIN..=MOT_DE_PASSE_MAX).contains(&longueur) {
            erreurs.push(FieldError::new(
                "mot_de_passe",
                format!("Entre {MOT_DE_PASSE_MIN} et {MOT_DE_PASSE_MAX} caractères ({longueur} reçus)."),
            ));
        }
        let role = Role::parse(role);
        if role.is_none() {
            erreurs.push(FieldError::new("role", "Valeurs possibles : admin, lecteur."));
        }
        match role {
            Some(role) if erreurs.is_empty() => Ok(Self { email, mot_de_passe, role }),
            _ => Err(ApiError::Validation(erreurs)),
        }
    }
}

/// Emails comparés sans tenir compte de la casse ni des espaces autour.
fn normaliser_email(email: &str) -> String {
    email.trim().to_lowercase()
}

fn objet(corps: Value, exemple: &str) -> Result<Map<String, Value>, ApiError> {
    match corps {
        Value::Object(objet) => Ok(objet),
        _ => Err(ApiError::requete_invalide(
            "Corps invalide",
            format!("Le corps doit être un objet JSON, ex. {exemple}."),
        )),
    }
}

fn texte_obligatoire(objet: &mut Map<String, Value>, champ: &str, erreurs: &mut Vec<FieldError>) -> Option<String> {
    match objet.remove(champ) {
        Some(Value::String(texte)) if !texte.trim().is_empty() => Some(texte),
        Some(Value::String(_)) => {
            erreurs.push(FieldError::new(champ, "Ne doit pas être vide."));
            None
        }
        Some(_) => {
            erreurs.push(FieldError::new(champ, "Texte attendu."));
            None
        }
        None => {
            erreurs.push(FieldError::new(champ, "Champ obligatoire."));
            None
        }
    }
}

fn champs_inconnus(reste: &Map<String, Value>, erreurs: &mut Vec<FieldError>) {
    erreurs.extend(reste.keys().map(|champ| FieldError::new(champ, "Champ inconnu.")));
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn empreinte_verifiee_et_jamais_egale_au_mot_de_passe() {
        let empreinte = hacher("correct horse battery");
        assert!(empreinte.starts_with("$argon2id$"));
        assert!(!empreinte.contains("correct horse battery"));
        assert!(verifier("correct horse battery", &empreinte));
        assert!(!verifier("Correct horse battery", &empreinte));
        assert!(!verifier("correct horse battery", "pas une empreinte"));
    }

    #[test]
    fn deux_hachages_du_meme_mot_de_passe_different() {
        assert_ne!(hacher("meme-mot-de-passe"), hacher("meme-mot-de-passe"));
    }

    #[test]
    fn nouvel_utilisateur_valide() {
        let nouveau = NouvelUtilisateur::depuis_json(
            json!({ "email": "  Alice@Exemple.FR ", "mot_de_passe": "douze-caract", "role": "admin" }),
        )
        .unwrap();
        assert_eq!(nouveau.email, "alice@exemple.fr");
        assert_eq!(nouveau.role, Role::Admin);
    }

    #[test]
    fn nouvel_utilisateur_liste_tous_les_champs_refuses() {
        let Err(ApiError::Validation(erreurs)) = NouvelUtilisateur::depuis_json(
            json!({ "email": "pas-un-email", "mot_de_passe": "court", "role": "superadmin", "age": 3 }),
        ) else {
            panic!("422 attendu");
        };
        let champs: Vec<&str> = erreurs.iter().map(|e| e.champ.as_str()).collect();
        assert_eq!(champs, ["age", "email", "mot_de_passe", "role"]);

        let Err(ApiError::Validation(erreurs)) = NouvelUtilisateur::depuis_json(json!({ "email": 3 })) else {
            panic!("422 attendu");
        };
        assert_eq!(erreurs.len(), 3);
    }

    #[test]
    fn connexion_exige_email_et_mot_de_passe() {
        assert!(Connexion::depuis_json(json!({ "email": "a@b.fr", "mot_de_passe": "x" })).is_ok());
        assert!(matches!(Connexion::depuis_json(json!({ "email": "a@b.fr" })), Err(ApiError::Validation(_))));
        assert!(matches!(Connexion::depuis_json(json!([])), Err(ApiError::RequeteInvalide { .. })));
    }
}
