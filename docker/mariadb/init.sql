-- Exécuté par l'image mariadb à la première création du volume (docker-entrypoint-initdb.d).

-- `#[sqlx::test]` se connecte à cette base pour y noter les bases de test qu'il crée.
CREATE DATABASE IF NOT EXISTS entreprises_tests;

-- Droits de l'utilisateur de l'API : sa base, la base des tests, les bases jetables de `#[sqlx::test]`
-- (`_sqlx_test_…`) et celle de `make acceptance` (`entreprises_acceptance`).
GRANT ALL PRIVILEGES ON `entreprises`.* TO 'entreprises'@'%';
GRANT ALL PRIVILEGES ON `entreprises\_tests`.* TO 'entreprises'@'%';
GRANT ALL PRIVILEGES ON `entreprises\_acceptance`.* TO 'entreprises'@'%';
GRANT ALL PRIVILEGES ON `\_sqlx\_test\_%`.* TO 'entreprises'@'%';
