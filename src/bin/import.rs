//! Import des données Sirene dans la base de l'API.
//!
//! ```text
//! cargo run --bin import                      # charge data/StockUniteLegale_100000_utf8.sql
//! cargo run --bin import -- autre_fichier.sql
//! cargo run --bin import -- --reset           # vide d'abord la base (suppressions comprises)
//! ```
//!
//! Si `mytable` a déjà été chargée avec le client `mariadb` et qu'aucun fichier n'est donné,
//! elle est recopiée directement. Relancer l'import ne recrée pas les SIREN supprimés
//! et n'écrase pas les modifications faites par `PATCH`.

use std::{path::PathBuf, time::Instant};

use anyhow::Context;
use entreprises_api::{MIGRATOR, config, import};

const FICHIER_DEFAUT: &str = "data/StockUniteLegale_100000_utf8.sql";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    config::charger_dotenv()?;
    let (options, fichiers): (Vec<String>, Vec<String>) =
        std::env::args().skip(1).filter(|a| a != "--").partition(|a| a.starts_with("--"));
    let reset = match options.as_slice() {
        [] => false,
        [option] if option == "--reset" => true,
        _ => anyhow::bail!("usage : import [--reset] [fichier.sql]"),
    };
    let fichier = fichiers.first().map(PathBuf::from);

    let database_url = config::database_url();
    let pool = config::connecter(&database_url).await?;
    let database_url = config::masquer_mot_de_passe(&database_url);
    MIGRATOR.run(&pool).await.context("échec des migrations")?;
    let debut = Instant::now();

    // Le fichier est lu avant tout --reset : un chemin erroné ne doit pas laisser une base vide.
    let source = if fichier.is_none() && import::table_source_presente(&pool).await? {
        println!("mytable déjà présente dans {database_url} : recopie directe.");
        None
    } else {
        let fichier = fichier.unwrap_or_else(|| FICHIER_DEFAUT.into());
        let contenu = tokio::fs::read_to_string(&fichier)
            .await
            .with_context(|| format!("lecture impossible de {}", fichier.display()))?;
        Some((fichier, contenu))
    };
    if reset {
        import::reinitialiser(&pool).await.context("échec de la réinitialisation")?;
        println!("Base vidée (entreprises et SIREN supprimés).");
    }
    if let Some((fichier, contenu)) = source {
        println!("Chargement de {} dans {database_url}…", fichier.display());
        import::charger_fichier(&pool, &contenu).await.context("échec du chargement du fichier SQL")?;
    }

    let rapport = import::recopier(&pool).await.context("échec de la recopie dans entreprises")?;
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM entreprises").fetch_one(&pool).await?;
    println!(
        "Import terminé en {:.1} s : {} lignes lues, {} ajoutées, {} déjà présentes (conservées), \
         {} ignorées (droit à l'oubli). Table entreprises : {total} lignes.",
        debut.elapsed().as_secs_f64(),
        rapport.lues,
        rapport.inserees,
        rapport.conservees,
        rapport.oubliees,
    );
    pool.close().await;
    Ok(())
}
