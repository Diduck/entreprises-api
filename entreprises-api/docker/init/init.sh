#!/bin/sh
# Lancé à chaque `docker compose up` : remplit la base puis s'arrête.
# L'import est relançable sans risque : il garde les modifications PATCH et ne recrée pas les SIREN supprimés.
set -e

# Les comptes de démonstration sont dans Keycloak (keycloak/realm-entreprises.json), plus dans la base.
import
