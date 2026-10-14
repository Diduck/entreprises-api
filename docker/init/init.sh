#!/bin/sh
# Lancé à chaque `docker compose up` : remplit la base puis s'arrête.
# L'import est relançable sans risque : il garde les modifications PATCH et ne recrée pas les SIREN supprimés.
set -e

import

# Comptes de démonstration (voir README). S'ils existent déjà, l'erreur affichée est sans conséquence.
MOT_DE_PASSE=admin-demo-mdp creer-utilisateur admin@demo.fr admin || true
MOT_DE_PASSE=lecteur-demo-mdp creer-utilisateur lecteur@demo.fr lecteur || true
