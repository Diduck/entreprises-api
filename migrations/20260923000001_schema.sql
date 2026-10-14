-- Schéma de l'API Entreprises (PRD § Import des données et schéma), pour MariaDB.
-- utf8mb4_bin : comparaisons et tris octet par octet, sensibles à la casse (la colonne `recherche`
-- est déjà normalisée en minuscules sans accents). Longueurs : celles de la validation du PATCH.

CREATE TABLE entreprises (
  siren                      BIGINT       NOT NULL PRIMARY KEY,  -- formaté sur 9 chiffres à la sortie
  denomination               VARCHAR(130),
  nom                        VARCHAR(100),
  prenom                     VARCHAR(20),
  activite_principale        VARCHAR(6),
  tranche_effectifs          CHAR(2)      NOT NULL,              -- stockée sur 2 caractères ("03")
  etat_administratif         CHAR(1)      NOT NULL CHECK (etat_administratif IN ('A', 'C')),
  date_creation              CHAR(10),                           -- AAAA-MM-JJ
  adresse_numero_voie        VARCHAR(10),
  adresse_type_voie          VARCHAR(4),
  adresse_libelle_voie       VARCHAR(100),
  adresse_code_postal        CHAR(5),
  adresse_commune            VARCHAR(100),
  date_derniere_modification CHAR(20),                           -- ISO 8601 UTC
  recherche                  TEXT         NOT NULL DEFAULT '',   -- dénomination + nom + prénom, minuscules sans accents
  INDEX idx_entreprises_activite (activite_principale)
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_bin;

-- Droit à l'oubli : seuls les SIREN supprimés sont conservés, pour qu'un réimport ne les recrée pas.
CREATE TABLE siren_supprimes (
  siren        BIGINT   NOT NULL PRIMARY KEY,
  supprime_le  CHAR(20) NOT NULL
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_bin;
