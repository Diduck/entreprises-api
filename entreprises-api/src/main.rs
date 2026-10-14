//! Serveur HTTP : configuration, pool MariaDB, migrations, arrêt propre.

use anyhow::Context;
use entreprises_api::{
    AppState, MIGRATOR, build_app,
    config::{self, Config},
};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    config::charger_dotenv()?;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("entreprises_api=info,tower_http=info")),
        )
        .init();

    let config = Config::depuis_env()?;
    let pool = config::connecter(&config.database_url).await?;
    MIGRATOR.run(&pool).await.context("échec des migrations")?;

    let entreprises: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM entreprises").fetch_one(&pool).await?;
    if entreprises == 0 {
        tracing::warn!("la table entreprises est vide : lancer `cargo run --bin import`");
    }
    let admins: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM utilisateurs WHERE role = 'admin'").fetch_one(&pool).await?;
    if admins == 0 {
        tracing::warn!("aucun compte admin : en créer un avec `make utilisateur EMAIL=… ROLE=admin`");
    }

    entreprises_api::utilisateurs::prechauffer();

    let listener = TcpListener::bind(&config.bind_addr)
        .await
        .with_context(|| format!("impossible d'écouter sur {}", config.bind_addr))?;
    tracing::info!("API prête sur http://{} ({entreprises} entreprises)", listener.local_addr()?);

    axum::serve(listener, build_app(AppState::new(pool.clone(), config.jwt)))
        .with_graceful_shutdown(signal_arret())
        .await?;
    pool.close().await;
    tracing::info!("arrêt propre terminé");
    Ok(())
}

/// Ctrl+C ou SIGTERM : le serveur n'accepte plus de connexion et termine les requêtes en cours.
async fn signal_arret() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("écoute de Ctrl+C");
    };
    #[cfg(unix)]
    let sigterm = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("écoute de SIGTERM")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let sigterm = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = sigterm => {},
    }
    tracing::info!("signal d'arrêt reçu : fin des requêtes en cours");
}
