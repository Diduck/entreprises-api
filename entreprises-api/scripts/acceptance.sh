#!/usr/bin/env bash
# Tests d'acceptation du PRD sur le jeu complet (99 999 entreprises), contre un vrai serveur HTTP.
#
#   ./scripts/acceptance.sh        (ou : make acceptance ; autre port : PORT=4000 ./scripts/acceptance.sh)
#
# Prérequis : MariaDB et Keycloak lancés (`make db`).
#
# Le script importe le fichier SQL dans une base temporaire (`entreprises_acceptance`, dans le conteneur),
# lance l'API dessus (port 3999 par défaut), obtient deux tokens Keycloak (comptes de démo), vérifie chaque critère E1 à E5,
# les droits, la pagination, la robustesse, le RGPD et les temps de réponse, puis supprime cette base.
# La base de démonstration `entreprises` n'est jamais touchée.
set -euo pipefail
cd "$(dirname "$0")/.."

PORT="${PORT:-3999}"
API="http://127.0.0.1:${PORT}/api/v1"
BASE="${API}/entreprises"
# Realm Keycloak du compose.yml et comptes de démo (keycloak/realm-entreprises.json).
KEYCLOAK="${OIDC_ISSUER:-http://127.0.0.1:8080/realms/entreprises}"
TMP="$(mktemp -d)"
BASE_SQL="entreprises_acceptance"
PASS=0
FAIL=0

# Rust installé par rustup mais shell pas encore rechargé.
command -v cargo >/dev/null || { [[ -f "$HOME/.cargo/env" ]] && source "$HOME/.cargo/env"; }

for outil in cargo curl jq docker; do
  command -v "$outil" >/dev/null || { echo "Outil manquant : $outil" >&2; exit 2; }
done

# sql <requête> : exécutée en root dans le conteneur MariaDB, résultat brut (sans en-tête)
sql() { docker compose -f ../compose.yml exec -T mariadb mariadb -uroot -proot-demo -N -B -e "$1"; }

vert() { printf '\033[32m%s\033[0m' "$1"; }
rouge() { printf '\033[31m%s\033[0m' "$1"; }
section() { printf '\n\033[1m%s\033[0m\n' "$1"; }

# check <libellé> <obtenu> <attendu>
check() {
  if [[ "$2" == "$3" ]]; then
    PASS=$((PASS + 1)); printf '  %s %s\n' "$(vert ✔)" "$1"
  else
    FAIL=$((FAIL + 1)); printf '  %s %s — obtenu « %s », attendu « %s »\n' "$(rouge ✘)" "$1" "$2" "$3"
  fi
}

# appel <méthode> <chemin> [token] [corps JSON] → STATUS, corps dans $TMP/body, headers dans $TMP/headers
appel() {
  : > "${TMP}/body"; : > "${TMP}/headers"
  local args=(-s -o "${TMP}/body" -D "${TMP}/headers" -w '%{http_code}' -X "$1")
  [[ -n "${3:-}" ]] && args+=(-H "Authorization: Bearer $3")
  [[ -n "${4:-}" ]] && args+=(-H 'Content-Type: application/json' --data "$4")
  STATUS="$(curl "${args[@]}" "${BASE}$2" || true)"
}
# token_keycloak <email> <mot de passe> : access token du client `swagger` (flux mot de passe, réservé aux scripts)
token_keycloak() {
  curl -s --max-time 5 -d client_id=swagger -d grant_type=password --data-urlencode "username=$1" \
    --data-urlencode "password=$2" "${KEYCLOAK}/protocol/openid-connect/token" | jq -r '.access_token // empty'
}
json() { jq -r "$1" "${TMP}/body" 2>/dev/null || echo "(corps non JSON)"; }
header() { grep -i "^$1:" "${TMP}/headers" | head -1 | cut -d' ' -f2- | tr -d '\r'; }
# total <query> : meta.total d'une recherche, ou « HTTP <statut> » si elle échoue
total() {
  appel GET "$1" "$LECTEUR"
  if [[ "$STATUS" == 200 ]]; then json .meta.total; else echo "HTTP $STATUS"; fi
}

# p95 <nb> <chemin> : 95e centile du temps de réponse en ms ; « KO » si une réponse n'est pas un 200
p95() {
  for _ in $(seq 1 "$1"); do
    curl -s -o /dev/null -w '%{http_code} %{time_total}\n' -H "Authorization: Bearer ${LECTEUR}" "${BASE}$2"
  done | sort -k2,2n | awk '
    $1 != 200 { ko = 1 }
    { t[NR] = $2 }
    END { if (ko) { printf "KO"; exit } i = int(NR * 0.95); if (i < 1) i = 1; printf "%.1f", t[i] * 1000 }'
}
# sous <valeur> <seuil> : 1 si la valeur (décimale) est sous le seuil
sous() { awk -v v="$1" -v s="$2" 'BEGIN { print (v != "KO" && v + 0 < s) ? 1 : 0 }'; }

arreter() {
  if [[ -n "${PID:-}" ]]; then
    kill "$PID" 2>/dev/null || true
    wait "$PID" 2>/dev/null || true
  fi
  sql "DROP DATABASE IF EXISTS ${BASE_SQL}" 2>/dev/null || true
  rm -rf "$TMP"
}
trap arreter EXIT

section "Préparation"
if curl -s -o /dev/null --max-time 1 "http://127.0.0.1:${PORT}/"; then
  echo "Le port ${PORT} est déjà utilisé : relancer avec PORT=<autre port> ./scripts/acceptance.sh" >&2
  exit 2
fi
sql "SELECT 1" >/dev/null 2>&1 || { echo "MariaDB ne répond pas : lancer \`make db\`" >&2; exit 2; }
ADMIN="$(token_keycloak admin@demo.fr admin-demo-mdp)"
LECTEUR="$(token_keycloak lecteur@demo.fr lecteur-demo-mdp)"
[[ "$ADMIN" == ey* && "$LECTEUR" == ey* ]] || { echo "Keycloak ne délivre pas de token (${KEYCLOAK}) : lancer \`make db\`" >&2; exit 2; }
sql "DROP DATABASE IF EXISTS ${BASE_SQL}; CREATE DATABASE ${BASE_SQL}"
cargo build --release --quiet --bins
export DATABASE_URL="mysql://entreprises:entreprises-demo@127.0.0.1:3308/${BASE_SQL}"
./target/release/import | sed 's/^/  /'
BIND_ADDR="127.0.0.1:${PORT}" OIDC_ISSUER="$KEYCLOAK" RUST_LOG=warn ./target/release/entreprises-api &
PID=$!
# Prêt quand CE serveur accepte le token admin.
for _ in $(seq 1 100); do
  kill -0 "$PID" 2>/dev/null || { echo "L'API de test s'est arrêtée au démarrage." >&2; exit 2; }
  STATUS="$(curl -s -o /dev/null -w '%{http_code}' -H "Authorization: Bearer $ADMIN" "${BASE}/036320596" || true)"
  [[ "$STATUS" == 200 ]] && break
  sleep 0.1
done
echo "  API de test sur http://127.0.0.1:${PORT}, tokens Keycloak admin et lecteur obtenus"

section "Authentification (Keycloak, OAuth2)"
# contenu <token> : partie centrale du JWT, décodée
contenu() { cut -d. -f2 <<< "$1" | tr '_-' '/+' | awk '{ while (length($0) % 4) $0 = $0 "="; print }' | base64 -d 2>/dev/null; }
check "token signé par la clé privée de Keycloak (RS256), avec un kid" \
  "$(cut -d. -f1 <<< "$LECTEUR" | tr '_-' '/+' | awk '{ while (length($0) % 4) $0 = $0 "="; print }' | base64 -d 2>/dev/null | jq -r '[.alg, (.kid | length > 0)] | join(" ")')" "RS256 true"
check "… émis par le realm, pour l'audience entreprises-api" \
  "$(contenu "$LECTEUR" | jq -r '[.iss, (.aud | if type == "array" then index("entreprises-api") != null else . == "entreprises-api" end)] | join(" ")')" "${KEYCLOAK} true"
check "… rôle lecteur, sans rôle admin" "$(contenu "$LECTEUR" | jq -r '.realm_access.roles | [index("lecteur") != null, index("admin") != null] | join(" ")')" "true false"
check "clé publique publiée (JWKS)" "$(curl -s "${KEYCLOAK}/protocol/openid-connect/certs" | jq -r '[.keys[] | select(.use == "sig")] | length > 0')" "true"
check "mauvais mot de passe → aucun token" "$(token_keycloak admin@demo.fr mauvais-mot-de-passe)" ""
falsifie="$(cut -d. -f1 <<< "$LECTEUR").$(cut -d. -f2 <<< "$ADMIN").$(cut -d. -f3 <<< "$LECTEUR")"
appel DELETE /036320596 "$falsifie"
check "contenu admin greffé sur une signature lecteur → 401 invalid_token" "$STATUS $(header www-authenticate)" '401 Bearer error="invalid_token"'
check "l'ancienne route de connexion n'existe plus → 404" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "${API}/tokens")" "404"

section "E1 — Retrouver une société par son identifiant"
appel GET /036320596 "$LECTEUR"
check "GET /036320596 → 200" "$STATUS" 200
check "… BOULANGERIE PATISSERIE L'EPI D'OR" "$(json .denomination)" "BOULANGERIE PATISSERIE L'EPI D'OR"
check "… tranche \"03\", état active, adresse et date de modification à null" \
  "$(json '[.tranche_effectifs, .etat_administratif, (.adresse | tostring), (.date_derniere_modification | tostring)] | join(" ")')" \
  "03 active null null"
check "… les 11 clés du contrat, dans l'ordre" "$(json 'keys_unsorted | join(",")')" \
  "siren,denomination,nom,prenom,activite_principale,tranche_effectifs,etat_administratif,date_creation,adresse,date_derniere_modification,links"
check "… Content-Type application/json" "$(header content-type)" "application/json"
appel GET /325175 "$LECTEUR"; court="$(jq -Sc . "${TMP}/body")"
appel GET /000325175 "$LECTEUR"; long="$(jq -Sc . "${TMP}/body")"
check "/325175 et /000325175 → même entreprise" "$court" "$long"
check "… siren \"000325175\"" "$(json .siren)" "000325175"
appel GET /12ab "$LECTEUR"
check "SIREN non numérique → 400 problem+json" "$STATUS $(header content-type)" "400 application/problem+json"
appel GET /1234567890 "$LECTEUR"
check "SIREN de plus de 9 chiffres → 400" "$STATUS" 400
appel GET /999999999 "$LECTEUR"
check "SIREN inconnu → 404" "$STATUS" 404

section "E2 — Chercher par code activité"
check "10.71C → 23 entreprises" "$(total '?activite_principale=10.71C')" 23
check "10.71C actives → 14" "$(total '?activite_principale=10.71C&etat_administratif=active')" 14
appel GET '?activite_principale=70.3C&limit=100' "$LECTEUR"
check "ancien code 70.3C trouvable tel quel (4 154)" "$STATUS $(json '.meta.total') $(json '[.data[].activite_principale] | unique | join(",")')" "200 4154 70.3C"
appel GET '?activite_principale=22.02&limit=100' "$LECTEUR"
check "ancien code 22.02 trouvable tel quel (82)" "$STATUS $(json '.meta.total') $(json '[.data[].activite_principale] | unique | join(",")')" "200 82 22.02"
check "10.71C,15.8C → union (23 + 171)" "$(total '?activite_principale=10.71C,15.8C')" 194

section "E3 — Chercher par dénomination"
check "boulangerie → 19 entreprises" "$(total '?denomination=boulangerie')" 19
check "boulangerie actives → 7" "$(total '?denomination=boulangerie&etat_administratif=active')" 7
check "Boulangérie (casse, accent) → 19" "$(total '?denomination=Boulang%C3%A9rie')" 19
check "BOULANGẼRIE (accent rare) → 19" "$(total '?denomination=BOULANG%E1%BA%BCRIE')" 19
appel GET '?denomination=janoyer' "$LECTEUR"
check "janoyer (nom) → 000325175" "$STATUS $(json '[.data[].siren] | join(",")')" "200 000325175"
appel GET '?denomination=bo' "$LECTEUR"
check "moins de 3 caractères → 400" "$STATUS" 400
appel GET '' "$LECTEUR"
check "aucun filtre → 400" "$STATUS" 400

section "Pagination et tri"
appel GET '?activite_principale=10.71C' "$LECTEUR"
check "page 1 : links.next présent" "$(json '.links.next')" "/api/v1/entreprises?activite_principale=10.71C&offset=20&limit=20"
appel GET '?activite_principale=10.71C&offset=20' "$LECTEUR"
check "dernière page : 3 résultats, pas de links.next" "$STATUS $(json '[(.data | length), (.links.next // "absent")] | join(" ")')" "200 3 absent"
appel GET '?denomination=boulangerie&limit=101' "$LECTEUR"
check "limit=101 → 400" "$STATUS" 400
appel GET '?activite_principale=70.3C&offset=9990&limit=20' "$LECTEUR"
check "offset + limit > 10 000 → 400" "$STATUS" 400
appel GET '?activite_principale=10.71C&offset=9223372036854775807&limit=1' "$LECTEUR"
check "offset géant (dépassement d'entier) → 400" "$STATUS" 400
appel GET '?activite_principale=10.71C&sort=-siren&limit=100' "$LECTEUR"
check "sort=-siren → ordre décroissant" "$STATUS $(json '[.data[].siren] == ([.data[].siren] | sort | reverse)')" "200 true"

section "Droits, méthodes et robustesse"
appel GET /036320596
check "sans token → 401" "$STATUS" 401
check "… WWW-Authenticate: Bearer" "$(header www-authenticate)" "Bearer"
appel GET /036320596 "mauvais-token"
check "token mal formé → 401" "$STATUS" 401
appel POST '' "$ADMIN" '{"denomination":"X"}'
check "POST → 405 (Allow: GET,HEAD)" "$STATUS $(header allow)" "405 GET,HEAD"
appel PUT /036320596 "$ADMIN" '{"denomination":"X"}'
check "PUT → 405 (Allow: GET,HEAD,PATCH,DELETE)" "$STATUS $(header allow)" "405 GET,HEAD,PATCH,DELETE"
appel OPTIONS /036320596
check "OPTIONS → 405, pas de CORS" "$STATUS $(header access-control-allow-origin)" "405 "
appel GET '?denomination=%00%00%00' "$LECTEUR"
check "denomination en octets NUL → 400" "$STATUS" 400
appel GET "?denomination=$(printf '_%.0s' $(seq 1 300))" "$LECTEUR"
check "denomination de 300 caractères → 400 (pas 500)" "$STATUS" 400
appel GET '?activite_principale=10.71C&sort=siren;DROP%20TABLE%20entreprises' "$LECTEUR"
check "injection dans sort → 400, table intacte" "$STATUS $(total '?activite_principale=10.71C')" "400 23"

section "Performance (p95 sur 100 requêtes, base intacte)"
e1="$(p95 100 /036320596)"
check "E1 < 50 ms (mesuré : ${e1} ms)" "$(sous "$e1" 50)" 1
e2="$(p95 100 '?activite_principale=10.71C')"
check "E2 < 200 ms (mesuré : ${e2} ms)" "$(sous "$e2" 200)" 1
e3="$(p95 100 '?denomination=boulangerie')"
check "E3 < 200 ms (mesuré : ${e3} ms)" "$(sous "$e3" 200)" 1

section "E5 — Modifier une entreprise"
appel PATCH /036320596 "$LECTEUR" '{"siren":"1"}'
check "token lecteur → 403 (même avec un corps invalide)" "$STATUS" 403
appel PATCH /036320596 "$ADMIN" \
  '{"adresse":{"numero_voie":"12","type_voie":"RUE","libelle_voie":"DE LA GARE","code_postal":"79190","commune":"SAUZE-VAUSSAIS"}}'
check "déménagement → 200" "$STATUS" 200
check "… adresse enregistrée" "$(json '.adresse.code_postal + " " + .adresse.commune')" "79190 SAUZE-VAUSSAIS"
check "… date_derniere_modification renseignée (UTC)" "$(json '.date_derniere_modification | test("^\\d{4}-\\d\\d-\\d\\dT\\d\\d:\\d\\d:\\d\\dZ$")')" true
appel PATCH /036320596 "$ADMIN" '{"denomination":"L'"'"'EPI D'"'"'OR"}'
check "changement de nom → 200" "$STATUS" 200
check "… seul ce champ change" "$(json '[.denomination, .activite_principale, .tranche_effectifs, .adresse.commune] | join("|")')" "L'EPI D'OR|10.71C|03|SAUZE-VAUSSAIS"
check "… la recherche suit le nouveau nom (boulangerie → 18)" "$(total '?denomination=boulangerie')" 18
appel PATCH /325175 "$ADMIN" '{"prenom":null}'
check "null vide le champ, absent inchangé" "$STATUS $(json '[(.prenom | tostring), .nom] | join(" ")')" "200 null JANOYER"
appel PATCH /036320596 "$ADMIN" '{"siren":"1","activite_principale":"70.3C","adresse":{"code_postal":"7919"},"prenom":"UN PRENOM BEAUCOUP TROP LONG"}'
check "siren, NAF, code postal, longueur → 422" "$STATUS" 422
check "… liste des champs refusés" "$(json '[.errors[].champ] | join(",")')" "activite_principale,adresse.code_postal,prenom,siren"
appel PATCH /036320596 "$ADMIN" '{"adresse":{"code_postal":"７９１９０"}}'
check "code postal en chiffres pleine chasse → 422" "$STATUS" 422
STATUS="$(curl -s -o "${TMP}/body" -w '%{http_code}' -X PATCH -H "Authorization: Bearer ${ADMIN}" --data '{"nom":"X"}' -H 'Content-Type:' "${BASE}/036320596")"
check "Content-Type absent → 415" "$STATUS" 415
appel PATCH /999999999 "$ADMIN" '{"nom":"X"}'
check "SIREN inconnu → 404" "$STATUS" 404

section "E4 — Supprimer une entreprise (droit à l'oubli)"
appel DELETE /036320596 "$LECTEUR"
check "token lecteur → 403" "$STATUS" 403
appel DELETE /036320596 "$ADMIN"
check "DELETE admin → 204 sans corps" "$STATUS $(wc -c < "${TMP}/body" | tr -d ' ')" "204 0"
appel GET /036320596 "$LECTEUR"
check "GET ensuite → 404" "$STATUS" 404
check "absente des recherches (10.71C → 22)" "$(total '?activite_principale=10.71C')" 22
check "RGPD : la ligne a disparu de la table entreprises" \
  "$(sql "SELECT COUNT(*) FROM ${BASE_SQL}.entreprises WHERE siren = 36320596")" 0
check "… seul le SIREN reste, dans siren_supprimes" \
  "$(sql "SELECT COUNT(*) FROM ${BASE_SQL}.siren_supprimes WHERE siren = 36320596")" 1
appel DELETE /999999999 "$ADMIN"
check "SIREN inconnu → 404" "$STATUS" 404
./target/release/import | sed 's/^/  réimport : /'
appel GET /036320596 "$LECTEUR"
check "après réimport, toujours 404" "$STATUS" 404
appel GET /325175 "$LECTEUR"
check "après réimport, les modifications PATCH sont conservées" "$STATUS $(json '.prenom | tostring')" "200 null"

printf '\n\033[1m%s réussis, %s échoués\033[0m\n' "$(vert "$PASS")" "$( ((FAIL)) && rouge "$FAIL" || echo 0)"
((FAIL == 0))
