.DEFAULT_GOAL := help
# compose.yml est à la racine du dépôt : il sert aussi à l'API Python (../python-api).
COMPOSE := docker compose -f ../compose.yml

.PHONY: help db db-stop db-shell import utilisateur utilisateurs-demo run run-all test acceptance lint reset

help: ## Liste les commandes
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk 'BEGIN { FS = ":.*## " } { printf "  make %-18s %s\n", $$1, $$2 }'

.env:
	cp .env.example .env

db: .env ## Démarre MariaDB (Docker), la remplit (import + comptes de démo) et lance l'API Python
	@# Docker Desktop fermé : on l'ouvre (macOS) et on attend qu'il réponde, 2 minutes au plus.
	@docker info >/dev/null 2>&1 || { \
	  echo "Démarrage de Docker Desktop…"; open -a Docker; \
	  for i in $$(seq 60); do docker info >/dev/null 2>&1 && break; sleep 2; done; \
	  docker info >/dev/null 2>&1 || { echo "Docker ne répond pas : ouvrir Docker Desktop à la main."; exit 1; }; }
	$(COMPOSE) up -d --wait

db-stop: ## Arrête MariaDB (les données restent dans le volume ; tout effacer : docker compose down -v à la racine du dépôt)
	$(COMPOSE) down

db-shell: ## Console SQL dans la base entreprises
	$(COMPOSE) exec mariadb mariadb -uentreprises -pentreprises-demo entreprises

import: .env ## Importe les 99 999 entreprises dans MariaDB (relançable sans risque)
	cargo run --release --bin import

utilisateur: .env ## Crée un compte : make utilisateur EMAIL=… ROLE=admin|lecteur (mot de passe demandé)
	@test -n "$(EMAIL)" -a -n "$(ROLE)" || { echo "usage : make utilisateur EMAIL=moi@exemple.fr ROLE=admin"; exit 2; }
	@cargo build --release --quiet --bin creer-utilisateur
	@printf 'Mot de passe (12 caractères au moins) : '; stty -echo; read -r mdp; stty echo; echo; \
	  printf '%s\n' "$$mdp" | ./target/release/creer-utilisateur "$(EMAIL)" "$(ROLE)"

utilisateurs-demo: .env ## Crée admin@demo.fr et lecteur@demo.fr (mots de passe de démo, voir README)
	@cargo build --release --quiet --bin creer-utilisateur
	@MOT_DE_PASSE=admin-demo-mdp ./target/release/creer-utilisateur admin@demo.fr admin || true
	@MOT_DE_PASSE=lecteur-demo-mdp ./target/release/creer-utilisateur lecteur@demo.fr lecteur || true

run: .env ## Lance l'API sur http://127.0.0.1:3000
	cargo run --release --bin entreprises-api

run-all: .env ## Lance tout : Docker (base, import, API Python) puis l'API Rust
	$(MAKE) db
	@echo "API Python prête sur http://127.0.0.1:8001/docs"
	$(MAKE) run

test: .env ## Tests unitaires et d'intégration (une base MariaDB jetable par test ; make db avant)
	DATABASE_URL="$$(grep '^TEST_DATABASE_URL=' .env | cut -d= -f2-)" cargo test

acceptance: ## Critères du PRD sur le jeu complet, contre un vrai serveur (base temporaire)
	./scripts/acceptance.sh

lint: ## Format, clippy sans avertissement
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings

reset: .env ## Repart des données d'origine (vide la base, suppressions comprises, puis réimporte ; serveur lancé ou non)
	cargo run --release --bin import -- --reset
