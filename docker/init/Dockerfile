# Image du service `init` : les commandes Rust `import` et `creer-utilisateur`, sans l'API.
# Contexte de build : la racine du projet (voir docker-compose.yml).

FROM rust:1.94-slim-trixie AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock build.rs ./
COPY migrations ./migrations
COPY src ./src
# Caches : les dépendances ne sont pas recompilées à chaque build.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --bin import --bin creer-utilisateur \
    && cp target/release/import target/release/creer-utilisateur /usr/local/bin/

FROM debian:trixie-slim
WORKDIR /app
COPY --from=build /usr/local/bin/import /usr/local/bin/creer-utilisateur /usr/local/bin/
COPY docker/init/init.sh /usr/local/bin/init.sh
CMD ["sh", "/usr/local/bin/init.sh"]
