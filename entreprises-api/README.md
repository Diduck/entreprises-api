# API REST Entreprises

API REST en Rust qui donne accès aux 99 999 entreprises de la base Sirene de l'INSEE : recherche, consultation, modification et suppression, avec connexion par JWT.

## Prérequis

- **Rust** 1.94 ou plus : <https://rustup.rs> (s'il est déjà installé, le mettre à jour : `rustup update stable`)
- **Docker Desktop** : <https://www.docker.com/products/docker-desktop/>

## Lancer le projet

Toutes les commandes se tapent **dans le dossier `entreprises-api`**, avec Docker Desktop ouvert.

```bash
make db    # Docker (../compose.yml) : démarre MariaDB, charge les 99 999 entreprises, crée les comptes de démo, lance l'API Python (../python-api)
make run   # lance l'API Rust
```

Ou les deux d'un coup : `make run-all`.

La toute première fois, `make db` compile les outils d'import dans Docker : compter quelques minutes. Ensuite, quelques secondes. Le chargement est refait à chaque `make db` sans rien écraser : les modifications et suppressions faites via l'API sont conservées.

L'API est prête quand le terminal affiche `API prête sur http://127.0.0.1:3000`. Laisser ce terminal ouvert ; **Ctrl+C** arrête l'API, `make db-stop` arrête la base.

## Utiliser l'API

Ouvrir **<http://127.0.0.1:3000/docs>** : c'est la documentation Swagger, où chaque route peut être essayée.

1. **Se connecter** : section *Authentification* → `POST /api/v1/tokens` → **Try it out** → **Execute**.
   Dans la réponse, plus bas, copier la valeur de `access_token`.
2. **S'authentifier** : bouton vert **Authorize** en haut de la page → coller le token → **Authorize** → **Close**.
3. **Essayer les routes** : ouvrir une route → **Try it out** → **Execute**.

Le token expire au bout de 15 minutes : refaire l'étape 1 en cas d'erreur 401.

Comptes de démonstration :

| Email | Mot de passe | Droits |
| --- | --- | --- |
| `lecteur@demo.fr` | `lecteur-demo-mdp` | consulter et rechercher |
| `admin@demo.fr` | `admin-demo-mdp` | tout, y compris modifier, supprimer et créer des comptes |

## Les routes

| Méthode | Route | Rôle | Ce qu'elle fait |
| --- | --- | --- | --- |
| POST | `/api/v1/tokens` | public | Se connecter, reçoit un token |
| GET | `/api/v1/entreprises/{siren}` | lecteur | Une entreprise par son SIREN, ex. `036320596` |
| GET | `/api/v1/entreprises?denomination=boulangerie` | lecteur | Recherche par nom et/ou code activité (`activite_principale=10.71C`) |
| PATCH | `/api/v1/entreprises/{siren}` | admin | Modifier une entreprise |
| DELETE | `/api/v1/entreprises/{siren}` | admin | Supprimer une entreprise (définitif) |
| POST | `/api/v1/utilisateurs` | admin | Créer un compte |
| GET | `/api/v1/utilisateurs/{id}` | admin | Lire un compte |

Les paramètres, exemples et codes d'erreur de chaque route sont détaillés dans Swagger.

## Commandes utiles

| Commande | Effet |
| --- | --- |
| `make` | Liste toutes les commandes |
| `make reset` | Remet les données d'origine (annule les modifications et suppressions) |
| `make test` | Lance les tests (la base doit tourner) |
| `make acceptance` | Vérifie tous les critères du cahier des charges sur un vrai serveur |

On peut aussi tester les routes depuis VS Code avec le fichier `requests.http` (extension REST Client).

Pour aller plus loin (configuration, fonctionnement interne, choix d'implémentation) : [docs/TECHNIQUE.md](docs/TECHNIQUE.md).
