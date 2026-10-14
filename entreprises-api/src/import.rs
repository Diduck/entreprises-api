//! Import des données Sirene : fichier SQL → table temporaire `mytable` → table `entreprises`.

use sqlx::{AssertSqlSafe, Connection, MySql, MySqlPool, QueryBuilder};

use crate::normalize;

/// Lignes insérées par requête (9 paramètres chacune, sous la limite de 65 535 des requêtes préparées).
const LOT: usize = 1_000;

/// Bilan d'une recopie.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Rapport {
    /// Lignes présentes dans `mytable`.
    pub lues: i64,
    /// Nouvelles entreprises.
    pub inserees: i64,
    /// Entreprises déjà présentes, laissées telles quelles (modifications `PATCH` préservées).
    pub conservees: i64,
    /// SIREN ignorés car supprimés au titre du droit à l'oubli.
    pub oubliees: i64,
}

/// Vrai si `mytable` existe déjà (chargée par exemple avec le client `mariadb`).
pub async fn table_source_presente(pool: &MySqlPool) -> sqlx::Result<bool> {
    let tables: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = DATABASE() AND table_name = 'mytable'",
    )
    .fetch_one(pool)
    .await?;
    Ok(tables > 0)
}

/// Instructions SQL exécutées par appel à `raw_sql`.
const INSTRUCTIONS_PAR_LOT: usize = 100;

/// Exécute le fichier SQL fourni (`CREATE TABLE mytable` + `INSERT`), les `INSERT` dans une seule transaction.
///
/// MariaDB valide implicitement la transaction à chaque `CREATE`/`DROP` : plutôt qu'un `BEGIN`, la connexion
/// passe en `autocommit = 0`, et une nouvelle transaction s'ouvre d'elle-même après le `CREATE TABLE`. Sans cela,
/// chaque `INSERT` serait validé seul (100 000 écritures sur disque). La connexion est retirée du pool : en cas
/// d'erreur, elle est fermée au lieu d'y retourner avec `autocommit = 0`.
pub async fn charger_fichier(pool: &MySqlPool, contenu: &str) -> sqlx::Result<()> {
    let mut connexion = pool.acquire().await?.detach();
    sqlx::raw_sql("DROP TABLE IF EXISTS mytable; SET autocommit = 0").execute(&mut connexion).await?;
    // Fichier de confiance fourni par l'équipe (open data INSEE), exécuté tel quel, par lots de 100 instructions.
    for lot in decouper_instructions(contenu, INSTRUCTIONS_PAR_LOT) {
        sqlx::raw_sql(AssertSqlSafe(lot)).execute(&mut connexion).await?;
    }
    sqlx::raw_sql("COMMIT; SET autocommit = 1").execute(&mut connexion).await?;
    connexion.close().await
}

/// Découpe un script SQL en lots d'instructions complètes, sans couper une chaîne entre apostrophes.
fn decouper_instructions(script: &str, par_lot: usize) -> Vec<&str> {
    let mut lots = Vec::new();
    let (mut debut, mut fin, mut instructions) = (0, 0, 0);
    let mut dans_une_chaine = false;
    for ligne in script.split_inclusive('\n') {
        fin += ligne.len();
        if ligne.bytes().filter(|&octet| octet == b'\'').count() % 2 == 1 {
            dans_une_chaine = !dans_une_chaine;
        }
        if !dans_une_chaine && ligne.trim_end().ends_with(';') {
            instructions += 1;
            if instructions == par_lot {
                lots.push(&script[debut..fin]);
                (debut, instructions) = (fin, 0);
            }
        }
    }
    if !script[debut..].trim().is_empty() {
        lots.push(&script[debut..]);
    }
    lots
}

/// Vide les entreprises et la liste des SIREN supprimés, sans supprimer le fichier :
/// un serveur lancé sur la même base voit tout de suite les données réimportées.
pub async fn reinitialiser(pool: &MySqlPool) -> sqlx::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM entreprises").execute(&mut *tx).await?;
    sqlx::query("DELETE FROM siren_supprimes").execute(&mut *tx).await?;
    tx.commit().await
}

#[derive(sqlx::FromRow)]
struct LigneSource {
    siren: i64,
    denomination: Option<String>,
    nom: Option<String>,
    prenom: Option<String>,
    activite_principale: Option<String>,
    tranche_effectifs: String,
    etat_administratif: String,
    date_creation: Option<String>,
}

/// Recopie `mytable` dans `entreprises` puis supprime `mytable`.
///
/// - tranche d'effectifs complétée sur 2 caractères ;
/// - colonne `recherche` calculée en Rust (minuscules, sans accents) ;
/// - SIREN de `siren_supprimes` ignorés (droit à l'oubli) ;
/// - SIREN déjà présents conservés : exclus de la lecture, jamais réécrits.
pub async fn recopier(pool: &MySqlPool) -> sqlx::Result<Rapport> {
    let mut tx = pool.begin().await?;
    let lues: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mytable").fetch_one(&mut *tx).await?;
    let oubliees: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM mytable WHERE siren IN (SELECT siren FROM siren_supprimes)")
            .fetch_one(&mut *tx)
            .await?;
    let lignes: Vec<LigneSource> = sqlx::query_as(
        "SELECT CAST(siren AS SIGNED) AS siren,
                NULLIF(TRIM(denomination_unite_legale), '') AS denomination,
                NULLIF(TRIM(nom_unite_legale), '') AS nom,
                NULLIF(TRIM(prenom_unite_legale), '') AS prenom,
                NULLIF(TRIM(activite_principale_unite_legale), '') AS activite_principale,
                CAST(tranche_effectifs_unite_legale AS CHAR) AS tranche_effectifs,
                CAST(etat_administratif_unite_legale AS CHAR) AS etat_administratif,
                NULLIF(TRIM(CAST(date_creation_unite_legale AS CHAR)), '') AS date_creation
         FROM mytable
         WHERE siren NOT IN (SELECT siren FROM siren_supprimes)
           AND siren NOT IN (SELECT siren FROM entreprises)
         ORDER BY siren",
    )
    .fetch_all(&mut *tx)
    .await?;

    let mut inserees = 0;
    for lot in lignes.chunks(LOT) {
        let mut requete = QueryBuilder::<MySql>::new(
            "INSERT INTO entreprises (siren, denomination, nom, prenom, activite_principale, \
             tranche_effectifs, etat_administratif, date_creation, recherche) ",
        );
        requete.push_values(lot, |mut valeurs, ligne| {
            valeurs
                .push_bind(ligne.siren)
                .push_bind(ligne.denomination.clone())
                .push_bind(ligne.nom.clone())
                .push_bind(ligne.prenom.clone())
                .push_bind(ligne.activite_principale.clone())
                .push_bind(normalize::pad_tranche(&ligne.tranche_effectifs))
                .push_bind(ligne.etat_administratif.clone())
                .push_bind(ligne.date_creation.clone())
                .push_bind(normalize::texte_recherche(
                    ligne.denomination.as_deref(),
                    ligne.nom.as_deref(),
                    ligne.prenom.as_deref(),
                ));
        });
        inserees += requete.build().execute(&mut *tx).await?.rows_affected() as i64;
    }

    sqlx::query("DROP TABLE mytable").execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Rapport { lues, inserees, conservees: lues - oubliees - inserees, oubliees })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoupe_sans_couper_les_chaines() {
        let script = "CREATE TABLE t(\n  a TEXT\n);\nINSERT INTO t VALUES ('L''EPI;\nD''OR');\nINSERT INTO t VALUES ('b');\nINSERT INTO t VALUES ('c');\n";
        let lots = decouper_instructions(script, 2);
        assert_eq!(lots.len(), 2);
        assert!(lots[0].ends_with("D''OR');\n"));
        assert_eq!(lots.concat(), script);
        assert_eq!(decouper_instructions("", 2), Vec::<&str>::new());
    }
}
