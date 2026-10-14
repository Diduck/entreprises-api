.DEFAULT_GOAL := help
.PHONY: help db db-stop db-shell import utilisateur utilisateurs-demo run test acceptance lint reset

help: ## Liste les commandes
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk 'BEGIN { FS = ":.*## " } { printf "  make %-18s %s\n", $$1, $$2 }'

.env:
	cp .env.example .env

db: ## Démarre MariaDB (Docker) et attend qu'elle soit prête
	docker compose up -d --wait

db-stop: ## Arrête MariaDB (les données restent dans le volume ; tout effacer : docker compose down -v)
	docker compose down

db-shell: ## Console SQL dans la base entreprises
	docker compose exec mariadb mariadb -uentreprises -pentreprises-demo entreprises

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

test: .env ## Tests unitaires et d'intégration (une base MariaDB jetable par test ; make db avant)
	DATABASE_URL="$$(grep '^TEST_DATABASE_URL=' .env | cut -d= -f2-)" cargo test

acceptance: ## Critères du PRD sur le jeu complet, contre un vrai serveur (base temporaire)
	./scripts/acceptance.sh

lint: ## Format, clippy sans avertissement
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings

reset: .env ## Repart des données d'origine (vide la base, suppressions comprises, puis réimporte ; serveur lancé ou non)
	cargo run --release --bin import -- --reset
