-- Comptes utilisateurs : connexion par email + mot de passe, puis JWT (voir src/auth.rs).

CREATE TABLE utilisateurs (
  id              BIGINT       NOT NULL AUTO_INCREMENT PRIMARY KEY,
  email           VARCHAR(254) NOT NULL UNIQUE,                  -- stocké en minuscules
  mot_de_passe    VARCHAR(255) NOT NULL,                         -- empreinte Argon2id (format PHC), jamais le mot de passe
  role            VARCHAR(7)   NOT NULL CHECK (role IN ('admin', 'lecteur')),
  cree_le         CHAR(20)     NOT NULL                          -- ISO 8601 UTC
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_bin;
