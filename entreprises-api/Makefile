.DEFAULT_GOAL := help
# compose.yml est à la racine du dépôt : il sert aussi à l'API Python (../python-api).
COMPOSE := docker compose -f ../compose.yml

.PHONY: help db db-stop db-shell import run run-all test acceptance lint reset

help: ## Liste les commandes
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk 'BEGIN { FS = ":.*## " } { printf "  make %-18s %s\n", $$1, $$2 }'

.env:
	cp .env.example .env

db: .env ## Lance tout dans Docker : MariaDB (remplie), Keycloak, API Rust (:3000) et API Python (:8001)
	@# Docker Desktop fermé : on l'ouvre (macOS) et on attend qu'il réponde, 2 minutes au plus.
	@docker info >/dev/null 2>&1 || { \
	  echo "Démarrage de Docker Desktop…"; open -a Docker; \
	  for i in $$(seq 60); do docker info >/dev/null 2>&1 && break; sleep 2; done; \
	  docker info >/dev/null 2>&1 || { echo "Docker ne répond pas : ouvrir Docker Desktop à la main."; exit 1; }; }
	$(COMPOSE) up -d --build --wait
	@echo "API Rust    : http://127.0.0.1:3000/docs"
	@echo "API Python  : http://127.0.0.1:8001/docs"
	@echo "Keycloak    : http://127.0.0.1:8080/admin (admin / admin-demo)"

db-stop: ## Arrête tous les conteneurs (les données restent dans le volume ; tout effacer : docker compose down -v à la racine du dépôt)
	$(COMPOSE) down

db-shell: ## Console SQL dans la base entreprises
	$(COMPOSE) exec mariadb mariadb -uentreprises -pentreprises-demo entreprises

import: .env ## Importe les 99 999 entreprises dans MariaDB (relançable sans risque)
	cargo run --release --bin import

run: .env ## Développement : API Rust hors Docker sur :3000 (arrête d'abord son conteneur ; make db la relance)
	@$(COMPOSE) stop rust-api 2>/dev/null || true
	cargo run --release --bin entreprises-api

run-all: db ## Comme make db (tout est dans Docker)

test: .env ## Tests unitaires et d'intégration (une base MariaDB jetable par test ; make db avant)
	DATABASE_URL="$$(grep '^TEST_DATABASE_URL=' .env | cut -d= -f2-)" cargo test

acceptance: ## Critères du PRD sur le jeu complet, contre un vrai serveur (base temporaire ; make db avant)
	./scripts/acceptance.sh

lint: ## Format, clippy sans avertissement
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings

reset: .env ## Repart des données d'origine (vide la base, suppressions comprises, puis réimporte ; serveur lancé ou non)
	cargo run --release --bin import -- --reset
