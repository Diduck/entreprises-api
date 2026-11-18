-- Les comptes sont désormais gérés par Keycloak (connexion unique OAuth2 / OpenID Connect) :
-- la table locale et ses empreintes de mots de passe ne servent plus.

DROP TABLE IF EXISTS utilisateurs;
