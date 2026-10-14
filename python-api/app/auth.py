"""Authentification par JWT, compatible avec l'API Rust.

Mêmes comptes (table `utilisateurs`, mots de passe Argon2id), même secret et mêmes règles
que `src/auth.rs` : un token obtenu sur l'une des deux API est valable sur l'autre.
"""

import os
import time

import jwt
from argon2 import PasswordHasher
from argon2.exceptions import InvalidHashError, VerificationError
from fastapi import Depends, HTTPException
from fastapi.security import HTTPAuthorizationCredentials, HTTPBearer
from sqlalchemy import text

from app.db import engine

# Même secret et même durée que l'API Rust : lus dans .env par compose.yml.
JWT_SECRET = os.environ["JWT_SECRET"]
JWT_DUREE_SECONDES = int(os.environ.get("JWT_DUREE_MINUTES", "15")) * 60
EMETTEUR = "entreprises-api"
ROLES = ("lecteur", "admin")

argon2 = PasswordHasher()
# Pour un email inconnu, on vérifie quand même une empreinte : la réponse prend le même temps,
# ce qui empêche de deviner quels comptes existent en chronométrant (comme l'API Rust).
EMPREINTE_FACTICE = argon2.hash("mot-de-passe-factice-jamais-valide")

# auto_error=False : sans token, on renvoie nous-mêmes un 401 (FastAPI renverrait 403).
bearer = HTTPBearer(auto_error=False, description="Token obtenu par `POST /api/v1/tokens`.")


def non_authentifie(detail: str) -> HTTPException:
    return HTTPException(status_code=401, detail=detail, headers={"WWW-Authenticate": "Bearer"})


def authentifier(email: str, mot_de_passe: str) -> dict | None:
    """Le compte si email et mot de passe sont bons, sinon `None` (sans dire lequel est faux)."""
    with engine.connect() as connexion:
        compte = connexion.execute(
            text("SELECT id, mot_de_passe, role FROM utilisateurs WHERE email = :email"),
            {"email": email.strip().lower()},
        ).mappings().first()
    try:
        argon2.verify(compte["mot_de_passe"] if compte else EMPREINTE_FACTICE, mot_de_passe)
    except (VerificationError, InvalidHashError):
        return None
    return dict(compte) if compte else None


def emettre(compte: dict) -> str:
    """Signe un token pour ce compte : mêmes champs que l'API Rust."""
    maintenant = int(time.time())
    claims = {
        "sub": str(compte["id"]),
        "role": compte["role"],
        "iss": EMETTEUR,
        "iat": maintenant,
        "exp": maintenant + JWT_DUREE_SECONDES,
    }
    return jwt.encode(claims, JWT_SECRET, algorithm="HS256")


def lecteur(credentials: HTTPAuthorizationCredentials | None = Depends(bearer)) -> dict:
    """Accès en lecture : token valide, rôle lecteur ou admin. Renvoie le contenu du token."""
    if credentials is None:
        raise non_authentifie("Token manquant : header `Authorization: Bearer <token>`.")
    try:
        claims = jwt.decode(
            credentials.credentials,
            JWT_SECRET,
            algorithms=["HS256"],
            issuer=EMETTEUR,
            options={"require": ["exp", "iss", "sub"]},
            leeway=0,
        )
    except jwt.InvalidTokenError:
        raise non_authentifie("Token invalide ou expiré : en demander un nouveau via POST /api/v1/tokens.")
    if claims.get("role") not in ROLES:
        raise non_authentifie("Token invalide : rôle inconnu.")
    return claims
