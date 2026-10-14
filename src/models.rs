//! Modèles : ligne de la base, entreprise JSON, paramètres de recherche et corps du `PATCH`.

use std::{fmt::Write as _, sync::LazyLock};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use validator::{Validate, ValidationError, ValidationErrors, ValidationErrorsKind};

use crate::{
    errors::{ApiError, FieldError},
    normalize,
};

/// Chemin de la ressource.
pub const BASE_PATH: &str = "/api/v1/entreprises";

// ---------------------------------------------------------------------------
// Entreprise
// ---------------------------------------------------------------------------

/// Ligne de la table `entreprises` (sans la colonne technique `recherche`).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EntrepriseRow {
    pub siren: i64,
    pub denomination: Option<String>,
    pub nom: Option<String>,
    pub prenom: Option<String>,
    pub activite_principale: Option<String>,
    pub tranche_effectifs: String,
    pub etat_administratif: String,
    pub date_creation: Option<String>,
    pub adresse_numero_voie: Option<String>,
    pub adresse_type_voie: Option<String>,
    pub adresse_libelle_voie: Option<String>,
    pub adresse_code_postal: Option<String>,
    pub adresse_commune: Option<String>,
    pub date_derniere_modification: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct Adresse {
    #[schema(example = "12")]
    pub numero_voie: Option<String>,
    #[schema(example = "RUE")]
    pub type_voie: Option<String>,
    #[schema(example = "DE LA GARE")]
    pub libelle_voie: Option<String>,
    #[schema(example = "79190")]
    pub code_postal: Option<String>,
    #[schema(example = "SAUZE-VAUSSAIS")]
    pub commune: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Links {
    #[serde(rename = "self")]
    #[schema(example = "/api/v1/entreprises/036320596")]
    pub self_: String,
}

/// Entreprise telle qu'exposée par l'API : 15 champs, les champs vides présents à `null`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Entreprise {
    /// 9 chiffres, zéros de tête compris.
    #[schema(example = "036320596", pattern = "^[0-9]{9}$")]
    pub siren: String,
    #[schema(example = "BOULANGERIE PATISSERIE L'EPI D'OR")]
    pub denomination: Option<String>,
    /// Nom de l'entrepreneur individuel.
    pub nom: Option<String>,
    pub prenom: Option<String>,
    /// Code NAF.
    #[schema(example = "10.71C")]
    pub activite_principale: Option<String>,
    /// Tranche d'effectifs INSEE sur 2 caractères (`NN` = non employeuse).
    #[schema(example = "03")]
    pub tranche_effectifs: String,
    #[schema(value_type = String, example = "active")]
    pub etat_administratif: &'static str,
    #[schema(example = "1963-01-01", format = Date)]
    pub date_creation: Option<String>,
    /// `null` tant qu'aucun champ d'adresse n'est renseigné.
    pub adresse: Option<Adresse>,
    /// Date du dernier `PATCH`, `null` si jamais modifiée.
    pub date_derniere_modification: Option<String>,
    pub links: Links,
}

impl From<EntrepriseRow> for Entreprise {
    fn from(row: EntrepriseRow) -> Self {
        let adresse = Adresse {
            numero_voie: row.adresse_numero_voie,
            type_voie: row.adresse_type_voie,
            libelle_voie: row.adresse_libelle_voie,
            code_postal: row.adresse_code_postal,
            commune: row.adresse_commune,
        };
        let adresse_vide = adresse.numero_voie.is_none()
            && adresse.type_voie.is_none()
            && adresse.libelle_voie.is_none()
            && adresse.code_postal.is_none()
            && adresse.commune.is_none();
        let siren = normalize::format_siren(row.siren);
        Self {
            links: Links { self_: format!("{BASE_PATH}/{siren}") },
            siren,
            denomination: row.denomination,
            nom: row.nom,
            prenom: row.prenom,
            activite_principale: row.activite_principale,
            tranche_effectifs: normalize::pad_tranche(&row.tranche_effectifs),
            etat_administratif: normalize::etat_vers_api(&row.etat_administratif),
            date_creation: row.date_creation,
            adresse: (!adresse_vide).then_some(adresse),
            date_derniere_modification: row.date_derniere_modification,
        }
    }
}

// ---------------------------------------------------------------------------
// Paramètre `fields`
// ---------------------------------------------------------------------------

/// Champs de premier niveau acceptés par `fields`.
pub const CHAMPS: [&str; 11] = [
    "siren",
    "denomination",
    "nom",
    "prenom",
    "activite_principale",
    "tranche_effectifs",
    "etat_administratif",
    "date_creation",
    "adresse",
    "date_derniere_modification",
    "links",
];

/// Réponse allégée : `?fields=siren,denomination`.
#[derive(Debug, Clone)]
pub struct Fields(Vec<&'static str>);

impl Fields {
    pub fn parse(brut: Option<&str>) -> Result<Option<Self>, ApiError> {
        let mut champs = Vec::new();
        let mut inconnus = Vec::new();
        for demande in liste(brut) {
            match CHAMPS.iter().find(|c| **c == demande) {
                Some(champ) if !champs.contains(champ) => champs.push(*champ),
                Some(_) => {}
                None => inconnus.push(demande),
            }
        }
        if !inconnus.is_empty() {
            return Err(ApiError::requete_invalide(
                "Paramètre invalide",
                format!(
                    "fields : champ(s) inconnu(s) {}. Champs possibles : {}.",
                    inconnus.join(", "),
                    CHAMPS.join(", ")
                ),
            ));
        }
        Ok((!champs.is_empty()).then_some(Self(champs)))
    }
}

/// Sérialise une entreprise en ne gardant que les champs demandés.
pub fn vers_json(entreprise: Entreprise, fields: Option<&Fields>) -> Value {
    let valeur = serde_json::to_value(entreprise).unwrap_or(Value::Null);
    match (fields, valeur) {
        (Some(Fields(champs)), Value::Object(objet)) => {
            Value::Object(objet.into_iter().filter(|(cle, _)| champs.contains(&cle.as_str())).collect())
        }
        (_, valeur) => valeur,
    }
}

// ---------------------------------------------------------------------------
// Recherche (E2, E3)
// ---------------------------------------------------------------------------

pub const LIMIT_DEFAUT: i64 = 20;
pub const LIMIT_MAX: i64 = 100;
/// `offset + limit` ne peut pas dépasser cette fenêtre.
pub const FENETRE_MAX: i64 = 10_000;
/// Longueur minimale d'un terme de recherche par dénomination.
pub const TERME_MIN: usize = 3;
/// Longueur maximale d'un terme saisi : dénomination (130) + nom (100) + prénom (20) séparés par des espaces.
pub const TERME_MAX: usize = 252;
/// Nombre maximal de valeurs distinctes séparées par des virgules dans un filtre.
pub const VALEURS_MAX: usize = 20;

/// Query string brute de `GET /entreprises` ; les valeurs sont validées par [`Recherche::depuis`].
#[derive(Debug, Default, Deserialize)]
pub struct SearchParams {
    pub activite_principale: Option<String>,
    pub denomination: Option<String>,
    pub etat_administratif: Option<String>,
    pub sort: Option<String>,
    pub offset: Option<String>,
    pub limit: Option<String>,
    pub fields: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColonneTri {
    Siren,
    Denomination,
    DateCreation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tri {
    pub colonne: ColonneTri,
    pub decroissant: bool,
}

/// Recherche validée.
#[derive(Debug, Clone)]
pub struct Recherche {
    /// Codes activité exacts (OU).
    pub activites: Vec<String>,
    /// Termes normalisés (minuscules, sans accents) cherchés dans `recherche` (OU).
    pub termes: Vec<String>,
    /// `A` et/ou `C` ; vide = toutes.
    pub etats: Vec<&'static str>,
    pub tri: Tri,
    pub offset: i64,
    pub limit: i64,
    pub fields: Option<Fields>,
    /// Paramètres repris dans `links` (hors `offset` et `limit`).
    params_liens: Vec<(&'static str, String)>,
}

impl Recherche {
    pub fn depuis(params: SearchParams) -> Result<Self, ApiError> {
        let mut activites: Vec<String> = Vec::new();
        for code in liste(params.activite_principale.as_deref()).map(str::to_uppercase) {
            if !activites.contains(&code) {
                activites.push(code);
            }
        }

        // Termes normalisés sans doublons (`Boulangerie,boulangérie` n'en fait qu'un) ; `denominations` garde la saisie pour les liens.
        let mut denominations: Vec<&str> = Vec::new();
        let mut termes: Vec<String> = Vec::new();
        for denomination in liste(params.denomination.as_deref()) {
            // Bornes mesurées sur la saisie ; le minimum l'est aussi après normalisation (« \0\0\0 » ne compte pas).
            let saisie = denomination.chars().count();
            let terme = normalize::fold(denomination);
            if saisie < TERME_MIN || terme.chars().count() < TERME_MIN {
                return Err(ApiError::requete_invalide(
                    "Recherche trop courte",
                    format!("denomination : « {denomination} » doit faire au moins {TERME_MIN} caractères."),
                ));
            }
            if saisie > TERME_MAX {
                return Err(ApiError::requete_invalide(
                    "Recherche trop longue",
                    format!("denomination : {TERME_MAX} caractères au plus par terme."),
                ));
            }
            if !termes.contains(&terme) {
                termes.push(terme);
                denominations.push(denomination);
            }
        }

        if activites.is_empty() && termes.is_empty() {
            return Err(ApiError::requete_invalide(
                "Filtre manquant",
                "Préciser au moins activite_principale ou denomination, ex. ?denomination=boulangerie.",
            ));
        }
        if activites.len() > VALEURS_MAX || termes.len() > VALEURS_MAX {
            return Err(ApiError::requete_invalide(
                "Paramètre invalide",
                format!("{VALEURS_MAX} valeurs distinctes au plus par filtre."),
            ));
        }

        let etats_saisis: Vec<&str> = liste(params.etat_administratif.as_deref()).collect();
        let mut etats = Vec::new();
        for etat in &etats_saisis {
            let code = normalize::etat_depuis_api(etat).ok_or_else(|| {
                ApiError::requete_invalide(
                    "Paramètre invalide",
                    format!("etat_administratif : « {etat} » inconnu. Valeurs possibles : active, cessee."),
                )
            })?;
            if !etats.contains(&code) {
                etats.push(code);
            }
        }

        let tri = parse_tri(params.sort.as_deref())?;
        let offset = parse_entier("offset", params.offset.as_deref(), 0)?;
        let limit = parse_entier("limit", params.limit.as_deref(), LIMIT_DEFAUT)?;
        if offset < 0 {
            return Err(ApiError::requete_invalide("Pagination invalide", "offset doit être positif ou nul."));
        }
        if !(1..=LIMIT_MAX).contains(&limit) {
            return Err(ApiError::requete_invalide(
                "Pagination invalide",
                format!("limit doit être compris entre 1 et {LIMIT_MAX}."),
            ));
        }
        // Écrit sans addition : un offset proche de i64::MAX ne doit pas déborder.
        if offset > FENETRE_MAX - limit {
            return Err(ApiError::requete_invalide(
                "Pagination invalide",
                format!(
                    "offset + limit ne peut pas dépasser {FENETRE_MAX} : affiner la recherche avec d'autres filtres."
                ),
            ));
        }
        let fields = Fields::parse(params.fields.as_deref())?;

        let mut params_liens = Vec::new();
        if !activites.is_empty() {
            params_liens.push(("activite_principale", activites.join(",")));
        }
        if !denominations.is_empty() {
            params_liens.push(("denomination", denominations.join(",")));
        }
        if !etats_saisis.is_empty() {
            params_liens.push(("etat_administratif", etats_saisis.join(",")));
        }
        if let Some(sort) = params.sort.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            params_liens.push(("sort", sort.to_owned()));
        }
        if let Some(Fields(champs)) = &fields {
            params_liens.push(("fields", champs.join(",")));
        }

        Ok(Self { activites, termes, etats, tri, offset, limit, fields, params_liens })
    }

    /// URI d'une page de cette recherche.
    fn lien(&self, offset: i64, limit: i64) -> String {
        let mut uri = format!("{BASE_PATH}?");
        for (cle, valeur) in &self.params_liens {
            let _ = write!(uri, "{cle}={}&", encoder(valeur));
        }
        let _ = write!(uri, "offset={offset}&limit={limit}");
        uri
    }

    /// Liens `self`, `next` (absent sur la dernière page) et `prev` (absent sur la première).
    pub fn liens(&self, total: i64) -> PageLinks {
        let suivant = self.offset + self.limit;
        let next = (suivant < total && suivant < FENETRE_MAX)
            .then(|| self.lien(suivant, self.limit.min(FENETRE_MAX - suivant)));
        let prev = (self.offset > 0).then(|| {
            let offset = (self.offset - self.limit).max(0);
            self.lien(offset, self.limit.min(self.offset))
        });
        PageLinks { self_: self.lien(self.offset, self.limit), next, prev }
    }
}

/// Réponse de recherche : `{ data, meta, links }`.
#[derive(Debug, Serialize, ToSchema)]
pub struct Page {
    /// Entreprises de la page ; avec `fields`, seuls les champs demandés.
    #[schema(value_type = Vec<Entreprise>)]
    pub data: Vec<Value>,
    pub meta: Meta,
    pub links: PageLinks,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Meta {
    /// Nombre total de résultats, toutes pages confondues.
    #[schema(example = 19)]
    pub total: i64,
    #[schema(example = 0)]
    pub offset: i64,
    #[schema(example = 20)]
    pub limit: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PageLinks {
    #[serde(rename = "self")]
    #[schema(example = "/api/v1/entreprises?denomination=boulangerie&offset=0&limit=20")]
    pub self_: String,
    /// Absent sur la dernière page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    /// Absent sur la première page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev: Option<String>,
}

/// Valeurs non vides d'un paramètre, séparées par des virgules.
fn liste(brut: Option<&str>) -> impl Iterator<Item = &str> {
    brut.unwrap_or_default().split(',').map(str::trim).filter(|v| !v.is_empty())
}

fn parse_tri(brut: Option<&str>) -> Result<Tri, ApiError> {
    let brut = brut.map(str::trim).filter(|s| !s.is_empty()).unwrap_or("siren");
    let (decroissant, nom) = match brut.strip_prefix('-') {
        Some(nom) => (true, nom),
        None => (false, brut),
    };
    let colonne = match nom {
        "siren" => ColonneTri::Siren,
        "denomination" => ColonneTri::Denomination,
        "date_creation" => ColonneTri::DateCreation,
        _ => {
            return Err(ApiError::requete_invalide(
                "Paramètre invalide",
                format!(
                    "sort : « {brut} » inconnu. Valeurs possibles : siren, denomination, date_creation, préfixe - pour décroissant."
                ),
            ));
        }
    };
    Ok(Tri { colonne, decroissant })
}

fn parse_entier(nom: &str, brut: Option<&str>, defaut: i64) -> Result<i64, ApiError> {
    match brut.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(defaut),
        Some(texte) => texte.parse().map_err(|_| {
            ApiError::requete_invalide("Pagination invalide", format!("{nom} : « {texte} » n'est pas un entier."))
        }),
    }
}

/// Encodage pour query string (RFC 3986) ; la virgule reste lisible.
fn encoder(valeur: &str) -> String {
    let mut sortie = String::with_capacity(valeur.len());
    for octet in valeur.bytes() {
        match octet {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b',' => sortie.push(octet as char),
            _ => {
                let _ = write!(sortie, "%{octet:02X}");
            }
        }
    }
    sortie
}

// ---------------------------------------------------------------------------
// PATCH (E5)
// ---------------------------------------------------------------------------

static RE_NAF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9]{2}\.[0-9]{2}[A-Z]$").expect("regex NAF"));
static RE_CODE_POSTAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9]{5}$").expect("regex code postal"));

/// Tranches d'effectifs INSEE (NN = unité non employeuse).
pub const TRANCHES: [&str; 16] =
    ["NN", "00", "01", "02", "03", "11", "12", "21", "22", "31", "32", "41", "42", "51", "52", "53"];

/// Champs non modifiables : 422 s'ils sont envoyés.
const NON_MODIFIABLES: [&str; 3] = ["siren", "date_derniere_modification", "links"];
/// Champs texte qu'un `null` vide.
const CHAMPS_VIDABLES: [&str; 5] = ["denomination", "nom", "prenom", "activite_principale", "date_creation"];
/// Champs texte obligatoires (colonnes NOT NULL) : `null` refusé.
const CHAMPS_OBLIGATOIRES: [&str; 2] = ["tranche_effectifs", "etat_administratif"];
const CHAMPS_ADRESSE: [&str; 5] = ["numero_voie", "type_voie", "libelle_voie", "code_postal", "commune"];

/// Corps d'un `PATCH` : champ absent → `None` (inchangé), `null` → `Some(None)` (vidé).
///
/// Au moins un champ ; `siren`, `date_derniere_modification` et `links` sont refusés (422).
#[derive(Debug, Default, Deserialize, Validate, ToSchema)]
#[schema(description = "Modification partielle : un champ absent reste inchangé, un champ à `null` est vidé. \
    `siren`, `date_derniere_modification` et `links` ne sont pas modifiables (422).")]
#[schema(example = json!({ "adresse": { "numero_voie": "12", "type_voie": "RUE", "libelle_voie": "DE LA GARE", "code_postal": "79190" } }))]
pub struct PatchEntreprise {
    #[serde(default, with = "::serde_with::rust::double_option")]
    #[validate(length(max = 130, message = "130 caractères au maximum."))]
    #[schema(value_type = Option<String>, max_length = 130)]
    pub denomination: Option<Option<String>>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    #[validate(length(max = 100, message = "100 caractères au maximum."))]
    #[schema(value_type = Option<String>, max_length = 100)]
    pub nom: Option<Option<String>>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    #[validate(length(max = 20, message = "20 caractères au maximum."))]
    #[schema(value_type = Option<String>, max_length = 20)]
    pub prenom: Option<Option<String>>,
    /// Code NAF rév. 2, ex. `10.71C` (mis en majuscules).
    #[serde(default, with = "::serde_with::rust::double_option")]
    #[validate(regex(path = *RE_NAF, message = "Format attendu : NN.NNL (NAF rév. 2), ex. 10.71C."))]
    #[schema(value_type = Option<String>, pattern = "^[0-9]{2}\\.[0-9]{2}[A-Z]$")]
    pub activite_principale: Option<Option<String>>,
    /// `NN`, `00`, `01`, `02`, `03`, `11`, `12`, `21`, `22`, `31`, `32`, `41`, `42`, `51`, `52` ou `53` (`3` → `03`).
    #[serde(default)]
    #[validate(custom(function = "valider_tranche"))]
    #[schema(value_type = String, required = false)]
    pub tranche_effectifs: Option<String>,
    /// `active` ou `cessee`.
    #[serde(default)]
    #[validate(custom(function = "valider_etat"))]
    #[schema(value_type = String, required = false)]
    pub etat_administratif: Option<String>,
    /// Date `AAAA-MM-JJ`.
    #[serde(default, with = "::serde_with::rust::double_option")]
    #[validate(custom(function = "valider_date"))]
    #[schema(value_type = Option<String>, format = Date)]
    pub date_creation: Option<Option<String>>,
    /// Champs d'adresse à changer, ou `null` pour effacer toute l'adresse.
    #[serde(default, with = "::serde_with::rust::double_option")]
    #[validate(nested)]
    #[schema(value_type = Option<PatchAdresse>)]
    pub adresse: Option<Option<PatchAdresse>>,
}

/// Adresse partielle : seuls les champs envoyés changent, `null` en vide un.
#[derive(Debug, Default, Deserialize, Validate, ToSchema)]
pub struct PatchAdresse {
    #[serde(default, with = "::serde_with::rust::double_option")]
    #[validate(length(max = 10, message = "10 caractères au maximum."))]
    #[schema(value_type = Option<String>, max_length = 10)]
    pub numero_voie: Option<Option<String>>,
    /// `RUE`, `AV`, `BD`…
    #[serde(default, with = "::serde_with::rust::double_option")]
    #[validate(length(max = 4, message = "4 caractères au maximum (RUE, AV, BD…)."))]
    #[schema(value_type = Option<String>, max_length = 4)]
    pub type_voie: Option<Option<String>>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    #[validate(length(max = 100, message = "100 caractères au maximum."))]
    #[schema(value_type = Option<String>, max_length = 100)]
    pub libelle_voie: Option<Option<String>>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    #[validate(regex(path = *RE_CODE_POSTAL, message = "5 chiffres attendus, ex. 79190."))]
    #[schema(value_type = Option<String>, pattern = "^[0-9]{5}$")]
    pub code_postal: Option<Option<String>>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    #[validate(length(max = 100, message = "100 caractères au maximum."))]
    #[schema(value_type = Option<String>, max_length = 100)]
    pub commune: Option<Option<String>>,
}

impl PatchEntreprise {
    /// Lit et valide le corps JSON ; renvoie tous les champs refusés d'un coup (422).
    pub fn depuis_json(corps: Value) -> Result<Self, ApiError> {
        let Value::Object(mut objet) = corps else {
            return Err(ApiError::requete_invalide(
                "Corps invalide",
                r#"Le corps doit être un objet JSON, ex. { "denomination": "L'EPI D'OR" }."#,
            ));
        };
        if objet.is_empty() {
            return Err(ApiError::Validation(vec![FieldError::new(
                "(corps)",
                "Au moins un champ à modifier est requis.",
            )]));
        }

        // 1. Noms et types des champs ; les champs refusés sont retirés avant la désérialisation.
        let mut erreurs = Vec::new();
        let mut rejetes = Vec::new();
        let mut rejetes_adresse = Vec::new();
        for (champ, valeur) in objet.iter_mut() {
            let accepte = match champ.as_str() {
                c if NON_MODIFIABLES.contains(&c) => {
                    erreurs.push(FieldError::new(c, "Ce champ n'est pas modifiable."));
                    false
                }
                c if CHAMPS_VIDABLES.contains(&c) => verifier_texte(c, valeur, true, &mut erreurs),
                c if CHAMPS_OBLIGATOIRES.contains(&c) => verifier_texte(c, valeur, false, &mut erreurs),
                "adresse" => match valeur {
                    Value::Null => true,
                    Value::Object(sous) if sous.is_empty() => {
                        erreurs.push(FieldError::new(
                            "adresse",
                            "Objet vide : préciser au moins un champ, ou null pour effacer l'adresse.",
                        ));
                        false
                    }
                    Value::Object(sous) => {
                        for (sous_champ, sous_valeur) in sous.iter_mut() {
                            let nom = format!("adresse.{sous_champ}");
                            let ok = if CHAMPS_ADRESSE.contains(&sous_champ.as_str()) {
                                verifier_texte(&nom, sous_valeur, true, &mut erreurs)
                            } else {
                                erreurs.push(FieldError::new(nom, "Champ inconnu."));
                                false
                            };
                            if !ok {
                                rejetes_adresse.push(sous_champ.clone());
                            }
                        }
                        true
                    }
                    _ => {
                        erreurs.push(FieldError::new(
                            "adresse",
                            "Objet attendu { numero_voie, type_voie, libelle_voie, code_postal, commune }, ou null.",
                        ));
                        false
                    }
                },
                c => {
                    erreurs.push(FieldError::new(c, "Champ inconnu."));
                    false
                }
            };
            if !accepte {
                rejetes.push(champ.clone());
            }
        }
        for champ in &rejetes {
            objet.remove(champ);
        }
        if let Some(Value::Object(sous)) = objet.get_mut("adresse") {
            for champ in &rejetes_adresse {
                sous.remove(champ);
            }
        }

        // 2. Saisies tolérées : tranche "3" → "03", code NAF en majuscules.
        if let Some(Value::String(tranche)) = objet.get_mut("tranche_effectifs") {
            *tranche = normalize::pad_tranche(tranche);
        }
        if let Some(Value::String(code)) = objet.get_mut("activite_principale") {
            *code = code.to_uppercase();
        }

        // 3. Valeurs (longueurs, formats) avec `validator`.
        let patch: Self = serde_json::from_value(Value::Object(objet))
            .map_err(|e| ApiError::Validation(vec![FieldError::new("(corps)", e.to_string())]))?;
        if let Err(validation) = patch.validate() {
            aplatir(&validation, "", &mut erreurs);
        }

        if erreurs.is_empty() {
            Ok(patch)
        } else {
            erreurs.sort_by(|a, b| a.champ.cmp(&b.champ));
            Err(ApiError::Validation(erreurs))
        }
    }

    /// Vrai si le texte de recherche doit être recalculé.
    pub fn touche_recherche(&self) -> bool {
        self.denomination.is_some() || self.nom.is_some() || self.prenom.is_some()
    }
}

/// Vérifie qu'une valeur est une chaîne non vide (ou `null` si vidable) et retire les espaces autour.
fn verifier_texte(champ: &str, valeur: &mut Value, vidable: bool, erreurs: &mut Vec<FieldError>) -> bool {
    match valeur {
        Value::String(texte) => {
            let nettoye = texte.trim();
            if nettoye.is_empty() {
                let message =
                    if vidable { "Valeur vide : envoyer null pour vider ce champ." } else { "Valeur vide refusée." };
                erreurs.push(FieldError::new(champ, message));
                return false;
            }
            if nettoye.len() != texte.len() {
                *texte = nettoye.to_owned();
            }
            true
        }
        Value::Null if vidable => true,
        Value::Null => {
            erreurs.push(FieldError::new(champ, "Ce champ est obligatoire : null refusé."));
            false
        }
        _ => {
            let message =
                if vidable { "Chaîne de caractères ou null attendue." } else { "Chaîne de caractères attendue." };
            erreurs.push(FieldError::new(champ, message));
            false
        }
    }
}

/// Convertit les erreurs de `validator` en liste plate `champ` / `message` (`adresse.code_postal`…).
fn aplatir(erreurs: &ValidationErrors, prefixe: &str, sortie: &mut Vec<FieldError>) {
    for (champ, genre) in erreurs.errors() {
        let nom = if prefixe.is_empty() { champ.to_string() } else { format!("{prefixe}.{champ}") };
        match genre {
            ValidationErrorsKind::Field(liste) => {
                for erreur in liste {
                    let message = erreur.message.as_deref().unwrap_or("Valeur refusée.");
                    sortie.push(FieldError::new(nom.clone(), message));
                }
            }
            ValidationErrorsKind::Struct(sous) => aplatir(sous, &nom, sortie),
            ValidationErrorsKind::List(elements) => {
                for (index, sous) in elements {
                    aplatir(sous, &format!("{nom}[{index}]"), sortie);
                }
            }
        }
    }
}

fn erreur(code: &'static str, message: &'static str) -> ValidationError {
    ValidationError::new(code).with_message(message.into())
}

fn valider_tranche(tranche: &str) -> Result<(), ValidationError> {
    if TRANCHES.contains(&tranche) {
        Ok(())
    } else {
        Err(erreur("tranche", "Valeurs possibles : NN, 00, 01, 02, 03, 11, 12, 21, 22, 31, 32, 41, 42, 51, 52, 53."))
    }
}

fn valider_etat(etat: &str) -> Result<(), ValidationError> {
    match normalize::etat_depuis_api(etat) {
        Some(_) => Ok(()),
        None => Err(erreur("etat", "Valeurs possibles : active, cessee.")),
    }
}

fn valider_date(date: &str) -> Result<(), ValidationError> {
    normalize::verifier_date(date).map_err(|message| erreur("date", message))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn champs_refuses(corps: Value) -> Vec<String> {
        match PatchEntreprise::depuis_json(corps) {
            Err(ApiError::Validation(erreurs)) => erreurs.into_iter().map(|e| e.champ).collect(),
            autre => panic!("422 attendu, obtenu {autre:?}"),
        }
    }

    #[test]
    fn patch_distingue_absent_et_null() {
        let patch = PatchEntreprise::depuis_json(json!({ "denomination": "L'EPI D'OR", "nom": null })).unwrap();
        assert_eq!(patch.denomination, Some(Some("L'EPI D'OR".into())));
        assert_eq!(patch.nom, Some(None));
        assert_eq!(patch.prenom, None);
        assert!(patch.touche_recherche());
    }

    #[test]
    fn patch_adresse_partielle() {
        let patch =
            PatchEntreprise::depuis_json(json!({ "adresse": { "code_postal": "79190", "commune": null } })).unwrap();
        let adresse = patch.adresse.unwrap().unwrap();
        assert_eq!(adresse.code_postal, Some(Some("79190".into())));
        assert_eq!(adresse.commune, Some(None));
        assert_eq!(adresse.libelle_voie, None);
    }

    #[test]
    fn patch_refuse_siren_et_champs_inconnus() {
        assert_eq!(champs_refuses(json!({ "siren": "1", "couleur": "bleu" })), ["couleur", "siren"]);
        assert_eq!(
            champs_refuses(json!({ "date_derniere_modification": "2026-01-01" })),
            ["date_derniere_modification"]
        );
    }

    #[test]
    fn patch_valide_formats_et_longueurs() {
        let refuses = champs_refuses(json!({
            "activite_principale": "70.3C",
            "adresse": { "code_postal": "7919", "type_voie": "AVENUE" },
            "prenom": "X".repeat(21),
            "denomination": "D".repeat(131),
            "tranche_effectifs": "99",
            "etat_administratif": "fermee",
            "date_creation": "2023-02-30",
        }));
        assert_eq!(
            refuses,
            [
                "activite_principale",
                "adresse.code_postal",
                "adresse.type_voie",
                "date_creation",
                "denomination",
                "etat_administratif",
                "prenom",
                "tranche_effectifs"
            ]
        );
    }

    #[test]
    fn patch_limites_acceptees() {
        let patch = PatchEntreprise::depuis_json(json!({
            "denomination": "D".repeat(130),
            "prenom": "É".repeat(20),
            "activite_principale": "10.71d",
            "tranche_effectifs": "3",
            "adresse": { "code_postal": "01000", "type_voie": "BD" },
        }))
        .unwrap();
        assert_eq!(patch.activite_principale, Some(Some("10.71D".into())));
        assert_eq!(patch.tranche_effectifs.as_deref(), Some("03"));
    }

    #[test]
    fn patch_refuse_types_null_et_vides() {
        assert_eq!(
            champs_refuses(json!({ "denomination": 12, "etat_administratif": null, "nom": "  ", "adresse": "rue" })),
            ["adresse", "denomination", "etat_administratif", "nom"]
        );
        assert_eq!(champs_refuses(json!({})), ["(corps)"]);
        assert!(matches!(PatchEntreprise::depuis_json(json!(["denomination"])), Err(ApiError::RequeteInvalide { .. })));
    }

    fn recherche(query: &str) -> Result<Recherche, ApiError> {
        let params: SearchParams = serde_json::from_value(
            query
                .split('&')
                .filter_map(|kv| kv.split_once('='))
                .map(|(k, v)| (k.to_owned(), Value::String(v.to_owned())))
                .collect::<serde_json::Map<_, _>>()
                .into(),
        )
        .unwrap();
        Recherche::depuis(params)
    }

    #[test]
    fn recherche_valide_les_parametres() {
        assert!(recherche("").is_err());
        assert!(recherche("denomination=ab").is_err());
        assert!(recherche("denomination=abc&limit=101").is_err());
        assert!(recherche("denomination=abc&limit=0").is_err());
        assert!(recherche("denomination=abc&offset=-1").is_err());
        assert!(recherche("denomination=abc&offset=9990&limit=20").is_err());
        assert!(recherche("denomination=abc&offset=9980&limit=20").is_ok());
        assert!(recherche("denomination=abc&sort=nom").is_err());
        assert!(recherche("denomination=abc&etat_administratif=fermee").is_err());
        assert!(recherche("denomination=abc&fields=siren,inconnu").is_err());
        let r = recherche("activite_principale=10.71c,15.8C&sort=-date_creation").unwrap();
        assert_eq!(r.activites, ["10.71C", "15.8C"]);
        assert_eq!(r.tri, Tri { colonne: ColonneTri::DateCreation, decroissant: true });
        assert_eq!((r.offset, r.limit), (0, LIMIT_DEFAUT));
    }

    #[test]
    fn liens_de_pagination() {
        let r = recherche("denomination=boulangerie").unwrap();
        let liens = r.liens(19);
        assert_eq!(liens.self_, "/api/v1/entreprises?denomination=boulangerie&offset=0&limit=20");
        assert_eq!((liens.next, liens.prev), (None, None));

        let r = recherche("denomination=boulangerie&offset=5&limit=5").unwrap();
        let liens = r.liens(19);
        assert_eq!(liens.next.as_deref(), Some("/api/v1/entreprises?denomination=boulangerie&offset=10&limit=5"));
        assert_eq!(liens.prev.as_deref(), Some("/api/v1/entreprises?denomination=boulangerie&offset=0&limit=5"));

        let r = recherche("denomination=l'épi d'or&offset=15&limit=5").unwrap();
        assert!(r.liens(19).next.is_none());
        assert!(r.liens(19).self_.contains("denomination=l%27%C3%A9pi%20d%27or"));
    }

    #[test]
    fn offset_geant_refuse_sans_debordement() {
        assert!(recherche("denomination=abc&offset=9223372036854775807&limit=1").is_err());
        assert!(recherche("denomination=abc&offset=9223372036854775800&limit=100").is_err());
        assert!(recherche("denomination=abc&offset=99999999999999999999").is_err());
    }

    #[test]
    fn termes_bornes_et_dedoublonnes() {
        assert!(recherche(&format!("denomination={}", "a".repeat(TERME_MAX))).is_ok());
        assert!(recherche(&format!("denomination={}", "a".repeat(TERME_MAX + 1))).is_err());
        assert!(recherche("denomination=\0\0\0").is_err());
        assert!(recherche("denomination=北").is_err());
        assert!(recherche("denomination=ßa").is_err());
        assert!(recherche(&format!("denomination={}", "Œ".repeat(130))).is_ok());
        assert!(recherche(&format!("denomination={}", "Œ".repeat(TERME_MAX + 1))).is_err());

        let r =
            recherche("denomination=Boulangerie,boulangérie,BOULANGERIE&activite_principale=10.71c,10.71C").unwrap();
        assert_eq!(r.termes, ["boulangerie"]);
        assert_eq!(r.activites, ["10.71C"]);
        let vingt_et_un: Vec<String> = (0..=VALEURS_MAX).map(|i| format!("{i:02}.00Z")).collect();
        assert!(recherche(&format!("activite_principale={}", vingt_et_un.join(","))).is_err());
        let doublons = vec!["10.71C"; 50].join(",");
        assert!(recherche(&format!("activite_principale={doublons}")).is_ok());
    }

    #[test]
    fn patch_refuse_les_chiffres_non_ascii() {
        assert_eq!(
            champs_refuses(json!({ "activite_principale": "١٠.٧١C", "adresse": { "code_postal": "７９１９０" } })),
            ["activite_principale", "adresse.code_postal"]
        );
    }
}
