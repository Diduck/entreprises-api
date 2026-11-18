# API REST Entreprises

API REST en Rust qui donne accès aux 99 999 entreprises de la base Sirene de l'INSEE : recherche, consultation, modification et suppression, avec connexion unique par Keycloak (OAuth2).

## Prérequis

- **Docker Desktop** : <https://www.docker.com/products/docker-desktop/>
- Pour développer et lancer les tests seulement : **Rust** 1.94 ou plus, <https://rustup.rs> (`rustup update stable` s'il est déjà installé)

## Lancer le projet

Toutes les commandes se tapent **dans le dossier `entreprises-api`**, avec Docker Desktop ouvert.

```bash
make db    # Docker (../compose.yml) : MariaDB (99 999 entreprises), Keycloak, cette API et l'API Python
```

La toute première fois, `make db` compile l'API et les outils d'import dans Docker : compter quelques minutes. Ensuite, quelques secondes. Le chargement est refait à chaque `make db` sans rien écraser : les modifications et suppressions faites via l'API sont conservées.

Tout est prêt quand `make db` affiche les adresses. `make db-stop` arrête tout ; `docker compose logs -f rust-api` (à la racine) affiche les journaux de l'API.

Pour développer : `make run` arrête le conteneur de l'API et la lance sur la machine (recompilation rapide, journaux dans le terminal). `make db` la remet dans Docker, recompilée.

## Utiliser l'API

Ouvrir **<http://127.0.0.1:3000/docs>** : c'est la documentation Swagger, où chaque route peut être essayée.

1. **Se connecter** : bouton vert **Authorize** en haut de la page → **Authorize** → page de connexion Keycloak
   → email et mot de passe → retour sur Swagger → **Close**. Le `client_id` (`swagger`) est déjà rempli.
2. **Essayer les routes** : ouvrir une route → **Try it out** → **Execute**.

La même connexion vaut pour l'API Python (<http://127.0.0.1:8001/docs>) : Keycloak garde la session, son
**Authorize** ne redemande pas le mot de passe. Le token expire au bout de 15 minutes : refaire **Authorize**
en cas d'erreur 401.

Comptes de démonstration :

| Email | Mot de passe | Droits |
| --- | --- | --- |
| `lecteur@demo.fr` | `lecteur-demo-mdp` | consulter et rechercher |
| `admin@demo.fr` | `admin-demo-mdp` | tout, y compris modifier et supprimer |

Les comptes se gèrent dans la console Keycloak, <http://127.0.0.1:8080/admin> (`admin` / `admin-demo`),
realm `entreprises` : *Users* → *Add user*, onglet *Credentials* pour le mot de passe, *Role mapping* pour le rôle
`lecteur` ou `admin`.

## Les routes

| Méthode | Route | Rôle | Ce qu'elle fait |
| --- | --- | --- | --- |
| GET | `/api/v1/entreprises/{siren}` | lecteur | Une entreprise par son SIREN, ex. `036320596` |
| GET | `/api/v1/entreprises?denomination=boulangerie` | lecteur | Recherche par nom et/ou code activité (`activite_principale=10.71C`) |
| PATCH | `/api/v1/entreprises/{siren}` | admin | Modifier une entreprise |
| DELETE | `/api/v1/entreprises/{siren}` | admin | Supprimer une entreprise (définitif) |

Les paramètres, exemples et codes d'erreur de chaque route sont détaillés dans Swagger.

## Commandes utiles

| Commande | Effet |
| --- | --- |
| `make` | Liste toutes les commandes |
| `make reset` | Remet les données d'origine (annule les modifications et suppressions) |
| `make test` | Lance les tests (la base doit tourner) |
| `make acceptance` | Vérifie tous les critères du cahier des charges sur un vrai serveur |

On peut aussi tester les routes depuis VS Code avec le fichier `requests.http` (extension REST Client).

Pour aller plus loin (configuration, clé privée / clé publique, fonctionnement interne, choix d'implémentation) : [docs/TECHNIQUE.md](docs/TECHNIQUE.md).

**Écart assumé avec le PRD** : `POST /api/v1/tokens` et les routes `/api/v1/utilisateurs` ont été retirées. Les comptes et la connexion sont dans Keycloak, partagé par les deux API.
