import os

import pandas as pd
from fastapi import FastAPI
from sqlalchemy import create_engine

# Même base MariaDB que l'API Rust (voir docker-compose.yml).
engine = create_engine(os.environ["DATABASE_URL"], pool_pre_ping=True)

app = FastAPI(title="API Python", version="0.1.0")


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


@app.get("/health")
def health():
    """Indique que l'API répond et que la base est joignable."""
    df = pd.read_sql("SELECT COUNT(*) AS total FROM entreprises", engine)
    return {"status": "ok", "entreprises": int(df.loc[0, "total"])}


@app.get("/stats/activites")
def entreprises_par_activite():
    """Nombre d'entreprises par code activité (APE/NAF), ex. `10.71C`."""
    return compter(lire_colonne("activite_principale"))


@app.get("/stats/creations")
def entreprises_par_annee_de_creation():
    """Nombre d'entreprises par année de création, de la plus ancienne à la plus récente."""
    dates = pd.to_datetime(lire_colonne("date_creation"), format="%Y-%m-%d", errors="coerce")
    return compter(dates.dt.year.astype("Int64").rename("annee_creation"), trier_par_valeur=True)
