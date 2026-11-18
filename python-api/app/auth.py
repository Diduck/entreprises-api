"""Authentification OAuth2 : tokens émis par Keycloak, vérifiés ici comme dans l'API Rust (`src/auth.rs`).

Keycloak signe chaque access token avec sa clé privée (RS256). Cette API ne connaît que la clé publique,
lue dans le JWKS de Keycloak (`OIDC_JWKS_URL`) : elle vérifie les signatures sans pouvoir en fabriquer.
Mêmes émetteur, audience et rôles que l'API Rust : une seule connexion Keycloak vaut pour les deux API.
"""

import os

import jwt
from fastapi import Depends, HTTPException
from fastapi.security import OAuth2AuthorizationCodeBearer

# Valeurs fixées dans compose.yml, identiques à celles de l'API Rust.
OIDC_ISSUER = os.environ.get("OIDC_ISSUER", "http://127.0.0.1:8080/realms/entreprises").rstrip("/")
OIDC_AUDIENCE = os.environ.get("OIDC_AUDIENCE", "entreprises-api")
OIDC_JWKS_URL = os.environ.get("OIDC_JWKS_URL", f"{OIDC_ISSUER}/protocol/openid-connect/certs")
# Client public du realm (keycloak/realm-entreprises.json), utilisé par Swagger UI.
CLIENT_SWAGGER = "swagger"

# Même règle que l'API Rust : les clés publiques sont gardées 5 minutes au plus (`lifespan`), puis
# retéléchargées. Une clé retirée de Keycloak (clé privée qui a fuité) n'est donc plus acceptée au bout de
# 5 minutes ; une clé inconnue (nouvelle clé) déclenche un téléchargement immédiat. Si Keycloak est
# injoignable à ce moment-là : 503.
# `cache_keys` reste à False : il ajouterait un second cache (lru_cache) qui n'expire jamais, où une clé
# retirée resterait acceptée jusqu'au redémarrage de l'API.
cles_keycloak = jwt.PyJWKClient(OIDC_JWKS_URL, cache_keys=False, lifespan=300, timeout=5)

# auto_error=False : sans token, on renvoie nous-mêmes un 401 au même format que l'API Rust.
keycloak = OAuth2AuthorizationCodeBearer(
    authorizationUrl=f"{OIDC_ISSUER}/protocol/openid-connect/auth",
    tokenUrl=f"{OIDC_ISSUER}/protocol/openid-connect/token",
    scheme_name="keycloak",
    description="Connexion Keycloak (client `swagger`, PKCE). Token RS256 accepté aussi par l'API Rust.",
    auto_error=False,
)


def non_authentifie(detail: str, token_fourni: bool) -> HTTPException:
    defi = 'Bearer error="invalid_token"' if token_fourni else "Bearer"
    return HTTPException(status_code=401, detail=detail, headers={"WWW-Authenticate": defi})


def verifier(token: str) -> dict:
    """Contenu du token si signature (RS256, clé publique de Keycloak), émetteur, audience et expiration sont bons."""
    try:
        cle = cles_keycloak.get_signing_key_from_jwt(token)
        return jwt.decode(
            token,
            cle.key,
            # RS256 uniquement : ni `none`, ni HS256 (qui permettrait de signer avec la clé publique).
            algorithms=["RS256"],
            issuer=OIDC_ISSUER,
            audience=OIDC_AUDIENCE,
            options={"require": ["exp", "iss", "aud", "sub"]},
            leeway=0,
        )
    except jwt.PyJWKClientConnectionError:
        raise HTTPException(status_code=503, detail="Keycloak injoignable : clés publiques indisponibles.")
    except (jwt.InvalidTokenError, jwt.PyJWKClientError):
        raise non_authentifie("Token invalide ou expiré : se reconnecter à Keycloak.", token_fourni=True)


def roles(claims: dict) -> list[str]:
    return claims.get("realm_access", {}).get("roles", [])


def lecteur(token: str | None = Depends(keycloak)) -> dict:
    """Accès en lecture : token Keycloak valide, rôle `lecteur` ou `admin`. Renvoie le contenu du token."""
    if token is None:
        raise non_authentifie("Token manquant : bouton Authorize de /docs (connexion Keycloak).", token_fourni=False)
    claims = verifier(token)
    if not {"lecteur", "admin"} & set(roles(claims)):
        raise HTTPException(status_code=403, detail="Ce compte n'a ni le rôle lecteur ni le rôle admin.")
    return claims
