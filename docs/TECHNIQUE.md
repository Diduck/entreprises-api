# API REST Entreprises — détails techniques

Complément du [README](../README.md) : fonctionnement interne, tests et choix d'implémentation.

## MariaDB dans Docker

`docker-compose.yml` lance trois services ; l'API Rust tourne sur la machine (`make run`).

| Service | Rôle |
| --- | --- |
| `mariadb` | La base, port hôte 3308 |
| `init` | Lance `import` puis crée les comptes de démo, et s'arrête. Image construite par `docker/init/Dockerfile` (commandes Rust compilées dans Docker), script `docker/init/init.sh`, fichier SQL monté depuis `data/`. Relancé à chaque `up` : l'import est idempotent (environ 3 s). |
| `python-api` | API Python (FastAPI + pandas), port hôte 8001, démarrée une fois `init` terminé avec succès |

| Commande | Effet |
| --- | --- |
| `make db` | Démarre les trois services et attend qu'ils soient prêts |
| `docker compose logs init` | Résultat du dernier import |
| `make db-shell` | Console SQL dans la base `entreprises` (`SHOW TABLES;`, `SELECT COUNT(*) FROM entreprises;`) |
| `make db-stop` | Arrête le conteneur ; les données restent dans le volume `mariadb-data` |
| `docker compose down -v` | Arrête et **efface** les données (volume supprimé) |
| `docker compose logs -f mariadb` | Journaux du serveur |

Au premier démarrage, `docker/mariadb/init.sql` crée la base des tests `entreprises_tests` et donne à l'utilisateur `entreprises` les droits sur les bases de test. Ce script ne tourne qu'à la création du volume : après l'avoir modifié, repartir de zéro avec `docker compose down -v`.

Pour un vrai compte admin (mot de passe demandé sans être affiché) : `make utilisateur EMAIL=moi@exemple.fr ROLE=admin`.

## Documentation OpenAPI

La spécification est générée à la compilation par [`utoipa`](https://docs.rs/utoipa) 6 à partir du code : les attributs `#[utoipa::path]` sur les handlers et les `#[derive(ToSchema)]` sur les modèles. Elle ne peut donc pas diverger des types Rust. La page est servie par `utoipa-swagger-ui` 10, dont les fichiers Swagger UI sont embarqués dans le binaire (feature `vendored`, aucun téléchargement au build). Les pages `/docs` et `/api-docs` sont publiques ; les routes de l'API restent protégées par JWT.

## Tester les routes

Quatre niveaux, du plus visuel au plus complet :

| Outil | Commande | Ce qu'il fait |
| --- | --- | --- |
| **Swagger UI** | `make run`, puis <http://127.0.0.1:3000/docs> | Documentation interactive : schémas, exemples et bouton « Try it out » sur chaque route. |
| **`requests.http`** | Ouvrir le fichier dans VS Code, cliquer « Send Request » | 46 requêtes prêtes : connexion, les exemples du contrat (E1 à E5), les erreurs, les droits et les comptes. Envoyer d'abord les deux « Connexion ». Extension [REST Client](https://marketplace.visualstudio.com/items?itemName=humao.rest-client), proposée à l'ouverture du projet. |
| **`make acceptance`** | `./scripts/acceptance.sh` | Importe le jeu complet dans une base MariaDB **temporaire** (`entreprises_acceptance`), crée deux comptes, lance un vrai serveur et vérifie 73 critères, puis nettoie. Couverture : connexion et JWT, totaux 23 / 14 / 19 / 7, codes 401 / 403 / 405 / 415 / 422, droit à l'oubli après réimport, robustesse, p95. La base `entreprises` n'est pas touchée. |
| **`make test`** | `cargo test` avec `DATABASE_URL=$TEST_DATABASE_URL` | 35 tests unitaires (normalisation, validation, pagination, JWT, mots de passe) et 49 tests d'intégration `axum-test` + `#[sqlx::test]` (38 pour E1 à E5, 11 pour l'authentification). Chaque test d'intégration part d'une base MariaDB neuve (`_sqlx_test_…`, créée puis supprimée par `#[sqlx::test]`) : MariaDB doit tourner (`make db`). S'y ajoutent 5 tests de la documentation (`tests/docs.rs`), qui tournent sans base : Swagger UI servie, les 7 routes documentées, la sécurité JWT et les erreurs problem+json. |

## Endpoints

| Méthode | URI | Rôle | Succès | Erreurs |
| --- | --- | --- | --- | --- |
| GET | `/api/v1/entreprises/{siren}?fields=` | lecteur | 200 | 400, 401, 404 |
| GET | `/api/v1/entreprises?activite_principale=&denomination=&etat_administratif=&sort=&offset=&limit=&fields=` | lecteur | 200 + page | 400, 401 |
| PATCH | `/api/v1/entreprises/{siren}` | admin | 200 | 400, 401, 403, 404, 415, 422 |
| DELETE | `/api/v1/entreprises/{siren}` | admin | 204 | 400, 401, 403, 404 |
| POST | `/api/v1/tokens` | public | 200 + JWT | 400, 401, 415, 422 |
| POST | `/api/v1/utilisateurs` | admin | 201 + `Location` | 400, 401, 403, 409, 415, 422 |
| GET | `/api/v1/utilisateurs/{id}` | admin | 200 | 400, 401, 403, 404 |

- `POST`, `PUT` et `OPTIONS` → 405 avec le header `Allow`.
- Toutes les erreurs sont en `application/problem+json` (RFC 9457), avec `errors[{champ, message}]` pour les 422.
- La recherche exige `activite_principale` ou `denomination`. Les filtres se cumulent (ET) et la virgule vaut OU dans un filtre, dans la limite de 20 valeurs distinctes.
- La dénomination saisie fait entre 3 et 252 caractères (la longueur maximale de dénomination + nom + prénom). Elle ignore la casse et les accents, et elle est cherchée aussi dans `nom` et `prenom`.
- Pagination : `offset` (0 par défaut), `limit` (20 par défaut, 100 au plus), `offset + limit ≤ 10 000`. La page est renvoyée avec `links.self`, `links.next` et `links.prev`.
- Tri : `sort=siren|denomination|date_creation`, avec un `-` devant pour l'ordre décroissant. Le SIREN départage les égalités.

## Configuration

Variables d'environnement, lues aussi depuis `.env` :

| Variable | Défaut | Rôle |
| --- | --- | --- |
| `DATABASE_URL` | `mysql://entreprises:entreprises-demo@127.0.0.1:3308/entreprises` | Base MariaDB (doit exister : créée par le conteneur) |
| `TEST_DATABASE_URL` | — | Base de `make test`, où `#[sqlx::test]` note ses bases jetables |
| `BIND_ADDR` | `127.0.0.1:3000` | Adresse d'écoute |
| `JWT_SECRET` | — (obligatoire) | Clé de signature des JWT, 32 caractères au moins : `openssl rand -hex 32` |
| `JWT_DUREE_MINUTES` | `15` | Durée de validité d'un token (1 à 1440) |
| `RUST_LOG` | `entreprises_api=info,tower_http=info` | Logs : méthode, chemin, statut, durée |

## Authentification (JWT)

1. **Comptes** : table `utilisateurs` (email, rôle `admin` ou `lecteur`). Le mot de passe n'est jamais stocké :
   seule son empreinte Argon2id l'est. Le premier admin se crée en ligne de commande (`make utilisateur`),
   les suivants via `POST /api/v1/utilisateurs` (admin). Mot de passe : 12 à 128 caractères.
2. **Connexion** : `POST /api/v1/tokens` avec `{ "email", "mot_de_passe" }` renvoie
   `{ "access_token", "token_type": "Bearer", "expires_in": 900 }` (noms de la RFC 6749).
   Un email inconnu et un mauvais mot de passe donnent la même réponse 401, dans le même temps.
3. **Requêtes** : header `Authorization: Bearer <access_token>`. Le JWT (HS256, signé avec `JWT_SECRET`) contient
   l'identifiant du compte (`sub`), son rôle et son expiration ; l'API le vérifie sans lire la base.
   Token absent → 401 ; mal formé, falsifié ou expiré → 401 `invalid_token` ; lecteur sur une écriture → 403.

Limites connues :

- Pas de refresh token ni de déconnexion côté serveur : un token reste valable jusqu'à son expiration (15 min),
  même si le compte est supprimé ou change de rôle. Changer `JWT_SECRET` invalide tous les tokens d'un coup.
- Pas de limite du nombre de tentatives de connexion : à ajouter avant une exposition publique.
- Les logs tracent l'identifiant du compte (`par=1` sur un `DELETE`), jamais son email.

## Import des données

`cargo run --bin import -- [--reset] [fichier.sql]` :

1. applique les migrations (`migrations/`) ;
2. exécute le fichier SQL (`mytable`) par blocs de 100 instructions, les `INSERT` dans une seule transaction (`autocommit = 0`, car le `CREATE TABLE` du fichier validerait un `BEGIN`) ;
3. recopie `mytable` dans `entreprises` :
   - tranche d'effectifs sur 2 caractères ;
   - colonne `recherche` en minuscules, sans accents ;
   - SIREN de `siren_supprimes` ignorés ;
   - SIREN déjà présents conservés (exclus de la recopie) ;
4. supprime `mytable`.

On peut aussi charger `mytable` avec le client `mariadb` du conteneur, puis lancer la recopie :

```bash
docker compose exec -T mariadb mariadb -uentreprises -pentreprises-demo entreprises < data/StockUniteLegale_100000_utf8.sql
cargo run --bin import        # sans argument : recopie la mytable déjà chargée
```

`build.rs`, généré par `sqlx migrate build-script`, recompile le projet quand une migration est ajoutée.

## Structure

```text
src/
├── main.rs         config, pool, migrations, serveur, arrêt propre (Ctrl+C / SIGTERM)
├── lib.rs          build_app(state) -> Router, partagé avec les tests
├── routes.rs       déclaration des routes
├── handlers.rs     E1 à E5
├── auth.rs         JWT (émission, vérification HS256), extracteurs Lecteur / Admin
├── utilisateurs.rs comptes : Argon2id, lecture / création en base, validation des corps JSON
├── models.rs       Entreprise, recherche (filtres, tri, pagination, fields), PatchEntreprise + validation
├── repository.rs   requêtes SQLx (QueryBuilder + push_bind, tri en liste blanche)
├── errors.rs       ApiError, conversion des rejets Axum, middleware problem+json
├── openapi.rs      spécification OpenAPI (utoipa), Swagger UI sur /docs
├── normalize.rs    SIREN sur 9 chiffres, tranche, A/C, retrait des accents (deunicode), dates
├── config.rs       variables d'environnement, pool MariaDB
├── import.rs       chargement du fichier SQL, recopie, réinitialisation
├── bin/import.rs   commande d'import
└── bin/creer-utilisateur.rs  création d'un compte en ligne de commande (premier admin)
tests/api.rs        tests d'intégration E1 à E5 et robustesse
tests/auth.rs       tests d'intégration connexion, JWT et comptes
tests/docs.rs       tests de la documentation OpenAPI (sans base)
scripts/acceptance.sh
requests.http
```

## Choix de mise en œuvre

- **Ordre des contrôles** : routage (404/405), puis authentification (401/403), puis validation (400/415/422). Un `POST` sans token sur `/entreprises` reçoit donc 405, et un lecteur reçoit 403 même avec un corps invalide.
- **Un seul format d'erreur** : `WithRejection` d'axum-extra convertit les rejets d'Axum (JSON invalide, Content-Type absent, paramètre répété) en problem+json, avec un détail en français. Un middleware couvre le reste (route inconnue, 405, timeout de 10 s en 408) et ajoute `instance` partout.
- **PATCH** : un champ absent n'est pas touché et un champ à `null` est vidé. Pour l'adresse, seuls les sous-champs envoyés changent et `"adresse": null` l'efface. Les 422 listent tous les champs refusés d'un coup. Deux saisies sont tolérées : `"3"` → `"03"` pour la tranche, `10.71c` → `10.71C` pour le code NAF.
- **RGPD** :
  - `DELETE` efface la ligne ; seul le SIREN reste, dans `siren_supprimes`. `make acceptance` le vérifie dans la base.
  - Contrairement à la version SQLite (`secure_delete` + checkpoint du WAL), rien ne garantit que les octets disparaissent tout de suite des fichiers InnoDB (pages, journaux redo et undo) : ils sont écrasés au fil de l'activité. Voir les écarts avec le PRD.
  - Les logs ne contiennent que le chemin, sans la query string, donc jamais un nom recherché.
- **Concurrence** : InnoDB verrouille par ligne. `PATCH` lit l'entreprise avec `SELECT … FOR UPDATE` : deux `PATCH` sur le même SIREN passent l'un après l'autre.
- **Collation** `utf8mb4_bin` : tris et comparaisons octet par octet, comme SQLite. `NULLS LAST` n'existant pas en MariaDB, le tri utilise `colonne IS NULL, colonne`.

Écarts assumés avec le PRD :

| Point | PRD | Choix | Raison |
| --- | --- | --- | --- |
| SIREN dans l'URI | `Path<u32>` + contrôle ≤ 999 999 999 | `Path<String>` + « chiffres uniquement, 9 au plus » | `u32` accepte `+123` et `0000000001` (10 chiffres), que le critère E1 demande de refuser (400) |
| DELETE sur un SIREN mal formé | Erreurs 401, 403, 404 | 400, comme E1 et E5 | Un SIREN illisible n'est pas « inconnu » ; ajouté dans l'onglet « Échanges de données » de l'Excel |
| Base de données | SQLite (`entreprises.db`) | MariaDB 11.8 dans Docker (`docker-compose.yml`) | Découverte de Docker |
| Documentation OpenAPI | `utoipa` 6 + `utoipa-scalar` 0.4 (`utoipa-swagger-ui` incompatible avec `utoipa` 6) | `utoipa` 6 + `utoipa-swagger-ui` 10 | `utoipa-swagger-ui` 10.0.1 (23 septembre 2026) dépend désormais de `utoipa` 6 ; Swagger UI demandé explicitement |
| Effacement physique (RGPD) | Plus aucune trace du nom sur disque après un DELETE | Ligne supprimée de la table ; octets écrasés plus tard par InnoDB | InnoDB n'a pas d'équivalent à `secure_delete` + checkpoint du WAL |

Réponses retenues aux questions ouvertes du PRD :

| Question | Choix |
| --- | --- |
| 415 quand `Content-Type` manque sur un PATCH | Renvoyé en problem+json ; ajouté dans l'onglet « Échanges de données » de l'Excel |
| Un PATCH peut-il remettre un ancien code (`70.3C`) ? | Non : seul le format NAF rév. 2 `NN.NNL` est accepté (422 sinon) |
| Un réimport écrase-t-il les modifications PATCH ? | Non : les modifications locales sont conservées |
