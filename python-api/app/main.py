import pandas as pd
from fastapi import APIRouter, Depends, FastAPI, Response
from fastapi.responses import HTMLResponse
from pydantic import BaseModel, Field

from app.auth import JWT_DUREE_SECONDES, authentifier, emettre, lecteur, non_authentifie
from app.db import engine

# `/docs` est servie plus bas, pour fixer la version de Swagger UI.
app = FastAPI(title="API Python", version="0.1.0", docs_url=None)

# Même version que l'API Rust (embarquée par utoipa-swagger-ui 10.0.1).
SWAGGER_UI = "https://cdn.jsdelivr.net/npm/swagger-ui-dist@5.32.6"


# Même configuration que utoipa : la mise en page « Standalone » apporte la barre du haut
# et le thème sombre (automatique si le système est en sombre, bouton pour basculer).
@app.get("/docs", include_in_schema=False, response_class=HTMLResponse)
def documentation():
    return f"""<!DOCTYPE html>
<html lang="fr">
<head>
  <meta charset="UTF-8">
  <title>{app.title}</title>
  <link rel="stylesheet" href="{SWAGGER_UI}/swagger-ui.css">
  <link rel="stylesheet" href="{SWAGGER_UI}/index.css">
  <link rel="icon" type="image/png" href="{SWAGGER_UI}/favicon-32x32.png">
</head>
<body>
  <div id="swagger-ui"></div>
  <script src="{SWAGGER_UI}/swagger-ui-bundle.js"></script>
  <script src="{SWAGGER_UI}/swagger-ui-standalone-preset.js"></script>
  <script>
    window.ui = SwaggerUIBundle({{
      url: "{app.openapi_url}",
      dom_id: "#swagger-ui",
      deepLinking: true,
      layout: "StandaloneLayout",
      presets: [SwaggerUIBundle.presets.apis, SwaggerUIStandalonePreset],
      plugins: [SwaggerUIBundle.plugins.DownloadUrl],
    }});
  </script>
</body>
</html>"""

# Même préfixe versionné que l'API Rust.
api_v1 = APIRouter(prefix="/api/v1")


def lire_colonne(colonne: str) -> pd.Series:
    """Toutes les valeurs de `colonne` : le regroupement est fait par pandas, pas en SQL."""
    return pd.read_sql(f"SELECT {colonne} FROM entreprises", engine)[colonne]


def compter(valeurs: pd.Series, trier_par_valeur: bool = False) -> list[dict]:
    """Nombre d'entreprises par valeur, du plus fréquent au moins fréquent (ou par valeur croissante).

    Les entreprises sans valeur sont comptées sous `null`.
    """
    comptes = valeurs.value_counts(dropna=False)
    if trier_par_valeur:
        comptes = comptes.sort_index(na_position="last")
    comptes = comptes.rename("nombre").reset_index()
    # NaN n'est pas du JSON valide : on le remplace par None (→ null).
    comptes = comptes.astype(object).where(comptes.notna(), None)
    return comptes.to_dict(orient="records")


class Connexion(BaseModel):
    email: str = Field(min_length=1, examples=["lecteur@demo.fr"])
    mot_de_passe: str = Field(min_length=1, examples=["lecteur-demo-mdp"])


class TokenEmis(BaseModel):
    access_token: str
    token_type: str = "Bearer"
    expires_in: int = Field(description="Durée de validité, en secondes.")


@api_v1.post(
    "/tokens",
    response_model=TokenEmis,
    responses={401: {"description": "Email ou mot de passe incorrect"}},
)
def connexion(corps: Connexion, response: Response):
    """Se connecter : renvoie un token, valable aussi sur l'API Rust (mêmes comptes)."""
    compte = authentifier(corps.email, corps.mot_de_passe)
    if compte is None:
        raise non_authentifie("Email ou mot de passe incorrect.")
    # Un token ne doit être gardé par aucun cache (RFC 6749 § 5.1).
    response.headers["Cache-Control"] = "no-store"
    return TokenEmis(access_token=emettre(compte), expires_in=JWT_DUREE_SECONDES)


@api_v1.get("/health")
def health():
    """Indique que l'API répond et que la base est joignable. Publique, sans token."""
    df = pd.read_sql("SELECT COUNT(*) AS total FROM entreprises", engine)
    return {"status": "ok", "entreprises": int(df.loc[0, "total"])}


@api_v1.get("/stats/activites", dependencies=[Depends(lecteur)])
def entreprises_par_activite():
    """Nombre d'entreprises par code activité (APE/NAF), ex. `10.71C`."""
    return compter(lire_colonne("activite_principale"))


@api_v1.get("/stats/creations", dependencies=[Depends(lecteur)])
def entreprises_par_annee_de_creation():
    """Nombre d'entreprises par année de création, de la plus ancienne à la plus récente."""
    dates = pd.to_datetime(lire_colonne("date_creation"), format="%Y-%m-%d", errors="coerce")
    return compter(dates.dt.year.astype("Int64").rename("annee_creation"), trier_par_valeur=True)


app.include_router(api_v1)
