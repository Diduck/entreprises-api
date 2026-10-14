//! Crée un compte utilisateur directement en base : c'est ainsi qu'on crée le premier admin.
//!
//! ```text
//! make utilisateur EMAIL=admin@exemple.fr ROLE=admin     # demande le mot de passe sans l'afficher
//! cargo run --bin creer-utilisateur -- admin@exemple.fr admin < fichier_contenant_le_mot_de_passe
//! MOT_DE_PASSE=… cargo run --bin creer-utilisateur -- lecteur@exemple.fr lecteur
//! ```
//!
//! Le mot de passe est lu dans `MOT_DE_PASSE`, sinon sur l'entrée standard (première ligne), jamais en argument :
//! un argument resterait visible dans l'historique du shell et dans la liste des processus.

use std::io::BufRead;

use anyhow::{Context, bail};
use entreprises_api::{
    MIGRATOR, config,
    errors::ApiError,
    utilisateurs::{self, NouvelUtilisateur},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    config::charger_dotenv()?;
    let arguments: Vec<String> = std::env::args().skip(1).filter(|a| a != "--").collect();
    let [email, role] = arguments.as_slice() else {
        bail!("usage : creer-utilisateur <email> <admin|lecteur>  (mot de passe : MOT_DE_PASSE ou entrée standard)");
    };
    let mot_de_passe = match std::env::var("MOT_DE_PASSE") {
        Ok(mot_de_passe) => mot_de_passe,
        Err(_) => {
            let mut ligne = String::new();
            std::io::stdin().lock().read_line(&mut ligne).context("lecture du mot de passe sur l'entrée standard")?;
            ligne.trim_end_matches(['\r', '\n']).to_owned()
        }
    };

    let nouveau = match NouvelUtilisateur::valider(email, mot_de_passe, role, Vec::new()) {
        Ok(nouveau) => nouveau,
        Err(ApiError::Validation(erreurs)) => {
            let details: Vec<String> = erreurs.iter().map(|e| format!("  - {} : {}", e.champ, e.message)).collect();
            bail!("compte refusé :\n{}", details.join("\n"));
        }
        Err(autre) => return Err(autre.into()),
    };

    let database_url = config::database_url();
    let pool = config::connecter(&database_url).await?;
    let database_url = config::masquer_mot_de_passe(&database_url);
    MIGRATOR.run(&pool).await.context("échec des migrations")?;
    let mot_de_passe = nouveau.mot_de_passe.clone();
    let empreinte = tokio::task::spawn_blocking(move || utilisateurs::hacher(&mot_de_passe)).await?;
    let Some(compte) = utilisateurs::creer(&pool, &nouveau, &empreinte).await? else {
        bail!("un compte existe déjà avec l'email {}", nouveau.email);
    };
    println!("Compte créé dans {database_url} : id {}, {}, rôle {}.", compte.id, compte.email, compte.role);
    pool.close().await;
    Ok(())
}
