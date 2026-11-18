# Image du service `rust-api` : le serveur HTTP de l'API Rust.
# Contexte de build : le dossier entreprises-api (voir compose.yml à la racine du dépôt).
# Les migrations sont embarquées dans le binaire : rien d'autre à copier dans l'image finale.

FROM rust:1.94-slim-trixie AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock build.rs ./
COPY migrations ./migrations
COPY src ./src
# Caches : les dépendances ne sont pas recompilées à chaque build.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --bin entreprises-api \
    && cp target/release/entreprises-api /usr/local/bin/

FROM debian:trixie-slim
WORKDIR /app
COPY --from=build /usr/local/bin/entreprises-api /usr/local/bin/
# Utilisateur sans droits : le serveur n'a besoin d'écrire nulle part.
RUN useradd --system --no-create-home api
USER api
EXPOSE 3000
CMD ["entreprises-api"]
