# Image du service `init` : la commande Rust `import`, sans l'API.
# Contexte de build : le dossier entreprises-api (voir compose.yml à la racine du dépôt).

FROM rust:1.94-slim-trixie AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock build.rs ./
COPY migrations ./migrations
COPY src ./src
# Caches : les dépendances ne sont pas recompilées à chaque build.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --bin import \
    && cp target/release/import /usr/local/bin/

FROM debian:trixie-slim
WORKDIR /app
COPY --from=build /usr/local/bin/import /usr/local/bin/
COPY docker/init/init.sh /usr/local/bin/init.sh
CMD ["sh", "/usr/local/bin/init.sh"]
