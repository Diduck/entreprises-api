import os

from sqlalchemy import create_engine

# Même base MariaDB que l'API Rust (voir compose.yml).
engine = create_engine(os.environ["DATABASE_URL"], pool_pre_ping=True)
