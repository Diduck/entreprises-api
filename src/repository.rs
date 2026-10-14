//! Requêtes SQLx. Toute valeur saisie passe par `bind`/`push_bind` ; le tri vient d'une liste blanche.

use sqlx::{MySql, MySqlPool, QueryBuilder};

use crate::{
    models::{ColonneTri, EntrepriseRow, PatchEntreprise, Recherche},
    normalize,
};

/// Colonnes lues pour construire une [`EntrepriseRow`].
macro_rules! colonnes {
    () => {
        "siren, denomination, nom, prenom, activite_principale, tranche_effectifs, \
         etat_administratif, date_creation, adresse_numero_voie, adresse_type_voie, \
         adresse_libelle_voie, adresse_code_postal, adresse_commune, date_derniere_modification"
    };
}

/// Horodatage ISO 8601 UTC calculé par MariaDB.
macro_rules! maintenant {
    () => {
        "DATE_FORMAT(UTC_TIMESTAMP(), '%Y-%m-%dT%H:%i:%sZ')"
    };
}

const SELECT_PAR_SIREN: &str = concat!("SELECT ", colonnes!(), " FROM entreprises WHERE siren = ?");

/// E1 — entreprise par SIREN.
pub async fn trouver(pool: &MySqlPool, siren: i64) -> sqlx::Result<Option<EntrepriseRow>> {
    sqlx::query_as(SELECT_PAR_SIREN).bind(siren).fetch_optional(pool).await
}

/// E2 / E3 — nombre total de résultats et page demandée.
pub async fn rechercher(pool: &MySqlPool, recherche: &Recherche) -> sqlx::Result<(i64, Vec<EntrepriseRow>)> {
    let mut compte = QueryBuilder::<MySql>::new("SELECT COUNT(*) FROM entreprises");
    ajouter_filtres(&mut compte, recherche);
    let total: i64 = compte.build_query_scalar().fetch_one(pool).await?;
    if recherche.offset >= total {
        return Ok((total, Vec::new()));
    }

    let mut page = QueryBuilder::<MySql>::new(concat!("SELECT ", colonnes!(), " FROM entreprises"));
    ajouter_filtres(&mut page, recherche);
    page.push(" ORDER BY ").push(clause_tri(recherche));
    page.push(" LIMIT ").push_bind(recherche.limit);
    page.push(" OFFSET ").push_bind(recherche.offset);
    let lignes = page.build_query_as().fetch_all(pool).await?;
    Ok((total, lignes))
}

/// Filtres cumulés (ET) ; plusieurs valeurs d'un même filtre sont combinées (OU).
fn ajouter_filtres(requete: &mut QueryBuilder<MySql>, recherche: &Recherche) {
    requete.push(" WHERE 1 = 1");
    if !recherche.activites.is_empty() {
        requete.push(" AND activite_principale IN (");
        let mut valeurs = requete.separated(", ");
        for code in &recherche.activites {
            valeurs.push_bind(code.clone());
        }
        requete.push(")");
    }
    if !recherche.termes.is_empty() {
        requete.push(" AND (");
        for (index, terme) in recherche.termes.iter().enumerate() {
            if index > 0 {
                requete.push(" OR ");
            }
            requete
                .push("recherche LIKE ")
                // Sans clause ESCAPE : le caractère d'échappement par défaut de MariaDB est `\`.
                .push_bind(format!("%{}%", normalize::echapper_like(terme)));
        }
        requete.push(")");
    }
    if !recherche.etats.is_empty() {
        requete.push(" AND etat_administratif IN (");
        let mut valeurs = requete.separated(", ");
        for etat in &recherche.etats {
            valeurs.push_bind(*etat);
        }
        requete.push(")");
    }
}

/// Clause `ORDER BY` issue d'une liste blanche ; le SIREN départage les égalités.
/// MariaDB ne connaît pas `NULLS LAST` : `colonne IS NULL` (0 puis 1) place les valeurs vides à la fin.
fn clause_tri(recherche: &Recherche) -> &'static str {
    match (recherche.tri.colonne, recherche.tri.decroissant) {
        (ColonneTri::Siren, false) => "siren ASC",
        (ColonneTri::Siren, true) => "siren DESC",
        (ColonneTri::Denomination, false) => "denomination IS NULL, denomination ASC, siren ASC",
        (ColonneTri::Denomination, true) => "denomination IS NULL, denomination DESC, siren ASC",
        (ColonneTri::DateCreation, false) => "date_creation IS NULL, date_creation ASC, siren ASC",
        (ColonneTri::DateCreation, true) => "date_creation IS NULL, date_creation DESC, siren ASC",
    }
}

/// E4 — supprime l'entreprise et garde son SIREN dans `siren_supprimes`. `false` si inconnue.
pub async fn supprimer(pool: &MySqlPool, siren: i64) -> sqlx::Result<bool> {
    let mut tx = pool.begin().await?;
    let supprime =
        sqlx::query("DELETE FROM entreprises WHERE siren = ?").bind(siren).execute(&mut *tx).await?.rows_affected();
    if supprime == 0 {
        return Ok(false);
    }
    sqlx::query(concat!(
        "INSERT INTO siren_supprimes (siren, supprime_le) VALUES (?, ",
        maintenant!(),
        ") ON DUPLICATE KEY UPDATE siren = siren"
    ))
    .bind(siren)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}

/// E5 — applique un `PATCH` validé. `None` si l'entreprise est inconnue.
pub async fn modifier(pool: &MySqlPool, siren: i64, patch: &PatchEntreprise) -> sqlx::Result<Option<EntrepriseRow>> {
    let mut tx = pool.begin().await?;
    // FOR UPDATE : la ligne reste verrouillée jusqu'au commit, un PATCH concurrent attend (lecture-modification-écriture
    // de la colonne `recherche`).
    let Some(actuelle) = sqlx::query_as::<_, EntrepriseRow>(concat!(
        "SELECT ",
        colonnes!(),
        " FROM entreprises WHERE siren = ? FOR UPDATE"
    ))
    .bind(siren)
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(None);
    };

    let mut requete =
        QueryBuilder::<MySql>::new(concat!("UPDATE entreprises SET date_derniere_modification = ", maintenant!()));
    let mut affecter = |colonne: &'static str, valeur: Option<String>| {
        requete.push(", ").push(colonne).push(" = ").push_bind(valeur);
    };

    if let Some(valeur) = &patch.denomination {
        affecter("denomination", valeur.clone());
    }
    if let Some(valeur) = &patch.nom {
        affecter("nom", valeur.clone());
    }
    if let Some(valeur) = &patch.prenom {
        affecter("prenom", valeur.clone());
    }
    if let Some(valeur) = &patch.activite_principale {
        affecter("activite_principale", valeur.clone());
    }
    if let Some(valeur) = &patch.tranche_effectifs {
        affecter("tranche_effectifs", Some(valeur.clone()));
    }
    if let Some(valeur) = &patch.etat_administratif {
        let code = normalize::etat_depuis_api(valeur).unwrap_or("A");
        affecter("etat_administratif", Some(code.to_owned()));
    }
    if let Some(valeur) = &patch.date_creation {
        affecter("date_creation", valeur.clone());
    }
    match &patch.adresse {
        None => {}
        // `"adresse": null` efface toute l'adresse.
        Some(None) => {
            for colonne in [
                "adresse_numero_voie",
                "adresse_type_voie",
                "adresse_libelle_voie",
                "adresse_code_postal",
                "adresse_commune",
            ] {
                affecter(colonne, None);
            }
        }
        Some(Some(adresse)) => {
            let champs = [
                ("adresse_numero_voie", &adresse.numero_voie),
                ("adresse_type_voie", &adresse.type_voie),
                ("adresse_libelle_voie", &adresse.libelle_voie),
                ("adresse_code_postal", &adresse.code_postal),
                ("adresse_commune", &adresse.commune),
            ];
            for (colonne, valeur) in champs {
                if let Some(valeur) = valeur {
                    affecter(colonne, valeur.clone());
                }
            }
        }
    }
    if patch.touche_recherche() {
        let denomination = patch.denomination.clone().unwrap_or(actuelle.denomination);
        let nom = patch.nom.clone().unwrap_or(actuelle.nom);
        let prenom = patch.prenom.clone().unwrap_or(actuelle.prenom);
        let texte = normalize::texte_recherche(denomination.as_deref(), nom.as_deref(), prenom.as_deref());
        affecter("recherche", Some(texte));
    }

    requete.push(" WHERE siren = ").push_bind(siren);
    requete.build().execute(&mut *tx).await?;

    let modifiee = sqlx::query_as(SELECT_PAR_SIREN).bind(siren).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(Some(modifiee))
}
