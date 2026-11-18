# API REST Entreprises — détails techniques

Complément du [README](../README.md) : fonctionnement interne, tests et choix d'implémentation.

## Docker : MariaDB, Keycloak, les deux API

`compose.yml`, à la racine du dépôt, lance cinq services. Pour développer l'API Rust hors conteneur : `make run`, qui arrête d'abord le service `rust-api`. Le Makefile l'appelle avec `docker compose -f ../compose.yml` ; à la main, taper les commandes `docker compose` depuis la racine du dépôt.

| Service | Rôle |
| --- | --- |
| `mariadb` | La base, port hôte 3308 |
| `keycloak` | Serveur d'identité OAuth2 / OpenID Connect (Keycloak 26.8, mode `start-dev`), port hôte 8080. Importe le realm `entreprises` depuis `../keycloak/` (rôles, client `swagger`, comptes de démo) à chaque création du conteneur, sans volume. `KC_HOSTNAME` fixe l'émetteur à `http://127.0.0.1:8080/realms/entreprises`, même vu du réseau Docker. Console : <http://127.0.0.1:8080/admin> (`admin` / `admin-demo`). |
| `init` | Lance `import` et s'arrête. Image construite par `docker/init/Dockerfile` (contexte : ce dossier) (commandes Rust compilées dans Docker), script `docker/init/init.sh`, fichier SQL monté depuis `data/`. Relancé à chaque `up` : l'import est idempotent (environ 3 s). |
| `rust-api` | Cette API, port hôte 3000. Image construite par `docker/api/Dockerfile` (binaire seul, utilisateur sans droits, migrations embarquées). Base sur `mariadb:3306`, clé publique de Keycloak sur `http://keycloak:8080`, émetteur attendu `http://127.0.0.1:8080/realms/entreprises`. Démarrée une fois `init` terminé et Keycloak prêt. Après une modification du code : `make db` (ou `docker compose up -d --build rust-api`). |
| `python-api` | API Python (FastAPI + pandas), port hôte 8001, démarrée une fois `init` terminé et Keycloak prêt. Routes `/api/v1/stats/*` réservées aux rôles lecteur et admin ; mêmes émetteur, audience et vérification RS256 que l'API Rust (`../python-api/app/auth.py`), clé publique lue sur `http://keycloak:8080` : un token Keycloak vaut pour les deux API. `/api/v1/health` reste publique. |

| Commande | Effet |
| --- | --- |
| `make db` | Construit les images, démarre les cinq services et attend qu'ils soient prêts (Keycloak : 20 à 40 s) |
| `docker compose logs -f rust-api` | Journaux de l'API Rust |
| `docker compose logs keycloak` | Journaux de Keycloak (import du realm) |
| `docker compose logs init` | Résultat du dernier import |
| `make db-shell` | Console SQL dans la base `entreprises` (`SHOW TABLES;`, `SELECT COUNT(*) FROM entreprises;`) |
| `make db-stop` | Arrête le conteneur ; les données restent dans le volume `mariadb-data` |
| `docker compose down -v` | Arrête et **efface** les données (volume supprimé) |
| `docker compose logs -f mariadb` | Journaux du serveur |

Au premier démarrage, `docker/mariadb/init.sql` crée la base des tests `entreprises_tests` et donne à l'utilisateur `entreprises` les droits sur les bases de test. Ce script ne tourne qu'à la création du volume : après l'avoir modifié, repartir de zéro avec `docker compose down -v`.

Pour un vrai compte : console Keycloak, voir [Authentification](#authentification-keycloak-oauth2--openid-connect).

## Documentation OpenAPI

La spécification est générée à la compilation par [`utoipa`](https://docs.rs/utoipa) 6 à partir du code : les attributs `#[utoipa::path]` sur les handlers et les `#[derive(ToSchema)]` sur les modèles. Elle ne peut donc pas diverger des types Rust. La page est servie par `utoipa-swagger-ui` 10, dont les fichiers Swagger UI sont embarqués dans le binaire (feature `vendored`, aucun téléchargement au build). Les pages `/docs` et `/api-docs` sont publiques ; les routes de l'API restent protégées (schéma OAuth2 `keycloak`, flux Authorization Code + PKCE, configuré dans Swagger UI avec le client `swagger`).

## Tester les routes

Quatre niveaux, du plus visuel au plus complet :

| Outil | Commande | Ce qu'il fait |
| --- | --- | --- |
| **Swagger UI** | `make run`, puis <http://127.0.0.1:3000/docs> | Documentation interactive : schémas, exemples et bouton « Try it out » sur chaque route. |
| **`requests.http`** | Ouvrir le fichier dans VS Code, cliquer « Send Request » | 46 requêtes prêtes : connexion, les exemples du contrat (E1 à E5), les erreurs, les droits et les comptes. Envoyer d'abord les deux « Connexion ». Extension [REST Client](https://marketplace.visualstudio.com/items?itemName=humao.rest-client), proposée à l'ouverture du projet. |
| **`make acceptance`** | `./scripts/acceptance.sh` | Importe le jeu complet dans une base MariaDB **temporaire** (`entreprises_acceptance`), obtient deux tokens Keycloak (comptes de démo), lance un vrai serveur et vérifie 69 critères, puis nettoie. Keycloak doit tourner (`make db`). Couverture : tokens Keycloak (RS256, émetteur, audience, rôles, falsification), totaux 23 / 14 / 19 / 7, codes 401 / 403 / 405 / 415 / 422, droit à l'oubli après réimport, robustesse, p95. La base `entreprises` n'est pas touchée. |
| **`make test`** | `cargo test` avec `DATABASE_URL=$TEST_DATABASE_URL` | 26 tests unitaires (normalisation, validation, pagination, lecture du JWKS) et 38 tests d'intégration `axum-test` + `#[sqlx::test]` pour E1 à E5 : chacun part d'une base MariaDB neuve (`_sqlx_test_…`, créée puis supprimée par `#[sqlx::test]`), MariaDB doit tourner (`make db`). S'y ajoutent, sans base ni Keycloak : 3 tests de rotation des clés (`tests/rotation.rs`, faux Keycloak local), 11 tests de l'authentification (`tests/auth.rs` : autre clé privée, `kid` inconnu, expiration, émetteur, audience, rôles, `alg: none`, HS256 signé avec la clé publique) et 6 tests de la documentation (`tests/docs.rs` : Swagger UI, les 4 routes, le schéma OAuth2, PKCE, problem+json). Keycloak y est remplacé par une paire de clés RSA de test (`tests/common/`). |

## Endpoints

| Méthode | URI | Rôle | Succès | Erreurs |
| --- | --- | --- | --- | --- |
| GET | `/api/v1/entreprises/{siren}?fields=` | lecteur | 200 | 400, 401, 404 |
| GET | `/api/v1/entreprises?activite_principale=&denomination=&etat_administratif=&sort=&offset=&limit=&fields=` | lecteur | 200 + page | 400, 401 |
| PATCH | `/api/v1/entreprises/{siren}` | admin | 200 | 400, 401, 403, 404, 415, 422 |
| DELETE | `/api/v1/entreprises/{siren}` | admin | 204 | 400, 401, 403, 404 |

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
| `OIDC_ISSUER` | `http://127.0.0.1:8080/realms/entreprises` | Realm Keycloak : émetteur (`iss`) exigé dans les tokens |
| `OIDC_AUDIENCE` | `entreprises-api` | Audience (`aud`) exigée dans les tokens |
| `OIDC_JWKS_URL` | `<OIDC_ISSUER>/protocol/openid-connect/certs` | Clés publiques de Keycloak (JWKS), en HTTP |
| `RUST_LOG` | `entreprises_api=info,tower_http=info` | Logs : méthode, chemin, statut, durée |

## Authentification (Keycloak, OAuth2 / OpenID Connect)

Les comptes et la connexion sont dans **Keycloak** (conteneur `keycloak` du `compose.yml`, realm `entreprises`
importé depuis [`keycloak/realm-entreprises.json`](../../keycloak/realm-entreprises.json)). Les deux API ne gèrent
plus aucun compte : elles vérifient seulement les tokens, d'où **une seule connexion pour les deux**.

1. **Clé privée / clé publique** : Keycloak génère une paire de clés RSA par realm. Il signe chaque access token
   (JWT) avec la **clé privée**, qu'il est le seul à détenir. Il publie la **clé publique** au format JWKS
   (`OIDC_JWKS_URL`). L'API la télécharge au démarrage : elle peut vérifier une signature, jamais en fabriquer une.
   Les clés sont rechargées dès qu'elles ont plus de **5 minutes**, et tout de suite quand un token arrive avec
   un `kid` inconnu (au plus une tentative toutes les 10 s). Une clé retirée de Keycloak n'est donc plus acceptée
   au bout de 5 minutes au plus. Si Keycloak est injoignable à ce moment-là : **503**, plutôt que de continuer à
   croire des clés peut-être retirées. Même règle côté Python (`PyJWKClient`, `lifespan=300`, `cache_keys=False`).
2. **Connexion** : flux OAuth2 *Authorization Code* + PKCE avec le client public `swagger` (bouton **Authorize**
   de `/docs` → page de connexion Keycloak → retour sur `/docs/oauth2-redirect.html`). La session Keycloak
   (cookie) évite de retaper le mot de passe sur la seconde API. Le client accepte aussi le flux « mot de passe »
   (`grant_type=password`), réservé aux scripts de démonstration (`requests.http`, `make acceptance`).
3. **Vérification** à chaque requête, sans appel à Keycloak : signature RS256 uniquement (ni `none`, ni HS256),
   `iss` = `OIDC_ISSUER`, `aud` contient `entreprises-api` (ajouté par un *audience mapper* du client), `exp`.
   Rôles lus dans `realm_access.roles` : `lecteur` (GET) ou `admin` (composite : inclut `lecteur`).
   Token absent → 401 ; mal formé, falsifié, expiré, autre émetteur ou autre audience → 401 `invalid_token` ;
   lecteur sur une écriture → 403 ; compte sans aucun de ces rôles → 403.
4. **Comptes** : console Keycloak <http://127.0.0.1:8080/admin> (`admin` / `admin-demo`), realm `entreprises`,
   *Users* → ajouter l'utilisateur, lui donner un mot de passe et le rôle `lecteur` ou `admin`.

**Si la clé privée de Keycloak a fuité** (quiconque la détient peut fabriquer des tokens admin) :

1. Console Keycloak → realm `entreprises` → *Realm settings* → onglet *Keys* → *Providers* → *Add provider* →
   `rsa-generated`, avec une *Priority* plus haute que celle de la clé actuelle (ex. 200) → *Save*.
   Keycloak signe aussitôt les nouveaux tokens avec cette nouvelle clé.
2. Même onglet : **supprimer** l'ancien provider `rsa-generated` (désactiver seulement *Active* ne suffit pas :
   la clé resterait publiée pour vérification).
3. Au plus 5 minutes après, les deux API ont rechargé le JWKS et refusent tout token signé par l'ancienne clé
   (401). Les utilisateurs se reconnectent (Authorize) et reçoivent un token signé par la nouvelle clé.

Testé par `tests/rotation.rs` (faux Keycloak dont le JWKS change, durée réduite à 1 s).

Limites connues :

- Une clé retirée reste acceptée **5 minutes au plus** (délai de rechargement) : c'est la fenêtre d'exposition.
- Un token reste valable jusqu'à son expiration (15 min, `accessTokenLifespan` du realm), même si le compte est
  désactivé ou change de rôle : l'API ne consulte pas Keycloak à chaque requête.
- Keycloak tourne en mode développement (`start-dev`, HTTP, base interne) : en production, HTTPS, base externe
  et hostname réel ; le JWKS en HTTPS demanderait alors d'activer TLS dans `reqwest`.
- La migration `20261118000001` supprime l'ancienne table `utilisateurs` (comptes locaux Argon2id).
- Les logs tracent l'identifiant Keycloak du compte (`par=<uuid>` sur un `DELETE`), jamais son email.

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
docker compose -f ../compose.yml exec -T mariadb mariadb -uentreprises -pentreprises-demo entreprises < data/StockUniteLegale_100000_utf8.sql
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
├── auth.rs         vérification des tokens Keycloak (RS256, clés publiques JWKS), extracteurs Lecteur / Admin
├── models.rs       Entreprise, recherche (filtres, tri, pagination, fields), PatchEntreprise + validation
├── repository.rs   requêtes SQLx (QueryBuilder + push_bind, tri en liste blanche)
├── errors.rs       ApiError, conversion des rejets Axum, middleware problem+json
├── openapi.rs      spécification OpenAPI (utoipa), Swagger UI sur /docs
├── normalize.rs    SIREN sur 9 chiffres, tranche, A/C, retrait des accents (deunicode), dates
├── config.rs       variables d'environnement, pool MariaDB
├── import.rs       chargement du fichier SQL, recopie, réinitialisation
├── bin/import.rs   commande d'import
tests/api.rs        tests d'intégration E1 à E5 et robustesse
tests/auth.rs       tests de la vérification des tokens (clé publique, émetteur, audience, rôles, attaques)
tests/rotation.rs   rotation des clés : clé retirée refusée, nouvelle clé acceptée, Keycloak injoignable → 503
tests/common/       faux Keycloak : paires de clés RSA réservées aux tests (tests/fixtures/cle-*.pem, jwks-*.json)
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
| Base de données | SQLite (`entreprises.db`) | MariaDB 11.8 dans Docker (`compose.yml`) | Découverte de Docker |
| Documentation OpenAPI | `utoipa` 6 + `utoipa-scalar` 0.4 (`utoipa-swagger-ui` incompatible avec `utoipa` 6) | `utoipa` 6 + `utoipa-swagger-ui` 10 | `utoipa-swagger-ui` 10.0.1 (23 septembre 2026) dépend désormais de `utoipa` 6 ; Swagger UI demandé explicitement |
| Effacement physique (RGPD) | Plus aucune trace du nom sur disque après un DELETE | Ligne supprimée de la table ; octets écrasés plus tard par InnoDB | InnoDB n'a pas d'équivalent à `secure_delete` + checkpoint du WAL |

Réponses retenues aux questions ouvertes du PRD :

| Question | Choix |
| --- | --- |
| 415 quand `Content-Type` manque sur un PATCH | Renvoyé en problem+json ; ajouté dans l'onglet « Échanges de données » de l'Excel |
| Un PATCH peut-il remettre un ancien code (`70.3C`) ? | Non : seul le format NAF rév. 2 `NN.NNL` est accepté (422 sinon) |
| Un réimport écrase-t-il les modifications PATCH ? | Non : les modifications locales sont conservées |
