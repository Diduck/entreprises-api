# API REST — Entreprises Sirene

Deux API sur la même base MariaDB (99 999 entreprises de la base Sirene de l'INSEE), avec **une seule connexion** : Keycloak (OAuth2 / OpenID Connect).

| Dossier | API | Adresse | Rôle |
| --- | --- | --- | --- |
| [entreprises-api/](entreprises-api/) | Rust (Axum) | <http://127.0.0.1:3000/docs> | Recherche, consultation, modification, suppression, comptes |
| [python-api/](python-api/) | Python (FastAPI + pandas) | <http://127.0.0.1:8001/docs> | Statistiques |
| [keycloak/](keycloak/) | Keycloak 26.8 | <http://127.0.0.1:8080/admin> | Comptes, rôles, connexion (console : `admin` / `admin-demo`) |

`compose.yml`, à la racine, lance tout dans Docker : la base MariaDB (et la remplit), Keycloak, l'API Rust et l'API Python.

## Lancer le projet

Prérequis : **Docker Desktop**. Rust (1.94 ou plus, <https://rustup.rs>) ne sert que pour développer et lancer les tests.

```bash
cd entreprises-api
make db   # tout dans Docker : base, import, Keycloak, API Rust, API Python
```

Ou, sans make, depuis la racine : `docker compose up -d --build --wait`. `make db-stop` arrête les conteneurs.

## Se connecter

Dans Swagger (l'une ou l'autre API) : bouton **Authorize** → **Authorize** → page de connexion Keycloak.
Sur la seconde API, Keycloak reconnaît la session : plus de mot de passe à taper.

Keycloak signe les tokens avec sa **clé privée** (RS256) ; les deux API les vérifient avec sa **clé publique**
(<http://127.0.0.1:8080/realms/entreprises/protocol/openid-connect/certs>). Les API ne stockent aucun compte
ni aucun secret.

| Email | Mot de passe | Droits |
| --- | --- | --- |
| `lecteur@demo.fr` | `lecteur-demo-mdp` | lecture |
| `admin@demo.fr` | `admin-demo-mdp` | tout |

## Routes de l'API Python

| Méthode | Route | Rôle | Ce qu'elle fait |
| --- | --- | --- | --- |
| GET | `/api/v1/health` | public | L'API répond et la base est joignable |
| GET | `/api/v1/stats/activites` | lecteur | Nombre d'entreprises par code activité |
| GET | `/api/v1/stats/creations` | lecteur | Nombre d'entreprises par année de création |

Les routes de l'API Rust et le détail de son fonctionnement : [entreprises-api/README.md](entreprises-api/README.md).

## Autres fichiers

- [keycloak/realm-entreprises.json](keycloak/realm-entreprises.json) : realm importé au démarrage (rôles `lecteur` / `admin`, client `swagger`, comptes de démo).
- [PRD.md](PRD.md) : cahier des charges de l'API Rust.
- `API_Valentin_AMORY.xlsx` : conception (modèle de données, échanges).
