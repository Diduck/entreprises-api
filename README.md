# API REST — Entreprises Sirene

Deux API sur la même base MariaDB (99 999 entreprises de la base Sirene de l'INSEE), avec les mêmes comptes et les mêmes tokens JWT.

| Dossier | API | Adresse | Rôle |
| --- | --- | --- | --- |
| [entreprises-api/](entreprises-api/) | Rust (Axum) | <http://127.0.0.1:3000/docs> | Recherche, consultation, modification, suppression, comptes |
| [python-api/](python-api/) | Python (FastAPI + pandas) | <http://127.0.0.1:8001/docs> | Statistiques |

`compose.yml`, à la racine, lance la base MariaDB, la remplit et démarre l'API Python. L'API Rust tourne sur la machine.

## Lancer le projet

Prérequis : **Rust** 1.94 ou plus (<https://rustup.rs>) et **Docker Desktop**.

```bash
cd entreprises-api
make run-all   # Docker (base, import, comptes de démo, API Python) puis l'API Rust
```

**Ctrl+C** arrête l'API Rust, `make db-stop` arrête les conteneurs.

## Se connecter

`POST /api/v1/tokens` sur l'une ou l'autre API, puis bouton **Authorize** dans Swagger. Un token obtenu sur une API est valable sur l'autre.

| Email | Mot de passe | Droits |
| --- | --- | --- |
| `lecteur@demo.fr` | `lecteur-demo-mdp` | lecture |
| `admin@demo.fr` | `admin-demo-mdp` | tout |

## Routes de l'API Python

| Méthode | Route | Rôle | Ce qu'elle fait |
| --- | --- | --- | --- |
| POST | `/api/v1/tokens` | public | Se connecter, reçoit un token |
| GET | `/api/v1/health` | public | L'API répond et la base est joignable |
| GET | `/api/v1/stats/activites` | lecteur | Nombre d'entreprises par code activité |
| GET | `/api/v1/stats/creations` | lecteur | Nombre d'entreprises par année de création |

Les routes de l'API Rust et le détail de son fonctionnement : [entreprises-api/README.md](entreprises-api/README.md).

## Autres fichiers

- [PRD.md](PRD.md) : cahier des charges de l'API Rust.
- `API_Valentin_AMORY.xlsx` : conception (modèle de données, échanges).
