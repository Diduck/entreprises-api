-- Extrait réel de data/StockUniteLegale_100000_utf8.sql (16 entreprises) pour les tests d'intégration.
-- 10.71C : 5 (3 actives) · 15.8C : 2 · « boulangerie » : 4 (2 actives) · 000325175 : entrepreneur JANOYER.
CREATE TABLE mytable(
   siren                            INTEGER  NOT NULL PRIMARY KEY 
  ,date_creation_unite_legale       DATE 
  ,prenom_unite_legale              VARCHAR(20)
  ,tranche_effectifs_unite_legale   VARCHAR(2) NOT NULL
  ,etat_administratif_unite_legale  VARCHAR(1) NOT NULL
  ,nom_unite_legale                 VARCHAR(29)
  ,denomination_unite_legale        VARCHAR(115)
  ,activite_principale_unite_legale VARCHAR(6)
);
INSERT INTO mytable(siren,date_creation_unite_legale,prenom_unite_legale,tranche_effectifs_unite_legale,etat_administratif_unite_legale,nom_unite_legale,denomination_unite_legale,activite_principale_unite_legale) VALUES (325175,'2000-09-26','THIERRY','NN','A','JANOYER',NULL,'32.12Z');
INSERT INTO mytable(siren,date_creation_unite_legale,prenom_unite_legale,tranche_effectifs_unite_legale,etat_administratif_unite_legale,nom_unite_legale,denomination_unite_legale,activite_principale_unite_legale) VALUES (5410220,'1954-12-25','GEORGES','NN','C','WATTEBLED',NULL,'22.02');
INSERT INTO mytable(siren,date_creation_unite_legale,prenom_unite_legale,tranche_effectifs_unite_legale,etat_administratif_unite_legale,nom_unite_legale,denomination_unite_legale,activite_principale_unite_legale) VALUES (5410345,NULL,'MICHEL','NN','C','DEBRAY',NULL,'79.06');
INSERT INTO mytable(siren,date_creation_unite_legale,prenom_unite_legale,tranche_effectifs_unite_legale,etat_administratif_unite_legale,nom_unite_legale,denomination_unite_legale,activite_principale_unite_legale) VALUES (5541040,NULL,'A','NN','C','PASCAL',NULL,NULL);
INSERT INTO mytable(siren,date_creation_unite_legale,prenom_unite_legale,tranche_effectifs_unite_legale,etat_administratif_unite_legale,nom_unite_legale,denomination_unite_legale,activite_principale_unite_legale) VALUES (5910344,'1959-12-25','MAURICE','NN','C','BOYAVAL',NULL,'15.8C');
INSERT INTO mytable(siren,date_creation_unite_legale,prenom_unite_legale,tranche_effectifs_unite_legale,etat_administratif_unite_legale,nom_unite_legale,denomination_unite_legale,activite_principale_unite_legale) VALUES (6680219,'1966-01-01',NULL,'11','A',NULL,'SYMARTIN','10.71C');
INSERT INTO mytable(siren,date_creation_unite_legale,prenom_unite_legale,tranche_effectifs_unite_legale,etat_administratif_unite_legale,nom_unite_legale,denomination_unite_legale,activite_principale_unite_legale) VALUES (7050057,NULL,NULL,'NN','C',NULL,'BOULANGERIE DIGNOISE','38.40');
INSERT INTO mytable(siren,date_creation_unite_legale,prenom_unite_legale,tranche_effectifs_unite_legale,etat_administratif_unite_legale,nom_unite_legale,denomination_unite_legale,activite_principale_unite_legale) VALUES (7140627,'1963-01-01','GERARD','NN','C','BOURJAC',NULL,'10.71C');
INSERT INTO mytable(siren,date_creation_unite_legale,prenom_unite_legale,tranche_effectifs_unite_legale,etat_administratif_unite_legale,nom_unite_legale,denomination_unite_legale,activite_principale_unite_legale) VALUES (7445281,'1994-12-25',NULL,'NN','C',NULL,'COPROPRIETAIRES','70.3C');
INSERT INTO mytable(siren,date_creation_unite_legale,prenom_unite_legale,tranche_effectifs_unite_legale,etat_administratif_unite_legale,nom_unite_legale,denomination_unite_legale,activite_principale_unite_legale) VALUES (7926785,'1994-12-25',NULL,'NN','A',NULL,'COPROPRIETE JOUVENEL N°26','81.10Z');
INSERT INTO mytable(siren,date_creation_unite_legale,prenom_unite_legale,tranche_effectifs_unite_legale,etat_administratif_unite_legale,nom_unite_legale,denomination_unite_legale,activite_principale_unite_legale) VALUES (13944129,'1994-12-25',NULL,'NN','A',NULL,'SYNDICAT DE COPROPRIETE BOULANGERIE','81.10Z');
INSERT INTO mytable(siren,date_creation_unite_legale,prenom_unite_legale,tranche_effectifs_unite_legale,etat_administratif_unite_legale,nom_unite_legale,denomination_unite_legale,activite_principale_unite_legale) VALUES (15851611,'1958-01-01',NULL,'NN','C',NULL,'SARL BOULANGERIE PATISSERIE LOOSLI','15.8C');
INSERT INTO mytable(siren,date_creation_unite_legale,prenom_unite_legale,tranche_effectifs_unite_legale,etat_administratif_unite_legale,nom_unite_legale,denomination_unite_legale,activite_principale_unite_legale) VALUES (16850422,'1965-01-01',NULL,'NN','C',NULL,'PATISSERIE GUILLEMARD SARL','10.71C');
INSERT INTO mytable(siren,date_creation_unite_legale,prenom_unite_legale,tranche_effectifs_unite_legale,etat_administratif_unite_legale,nom_unite_legale,denomination_unite_legale,activite_principale_unite_legale) VALUES (24044943,'1998-10-01','ALI','NN','A','DALALI',NULL,'10.71C');
INSERT INTO mytable(siren,date_creation_unite_legale,prenom_unite_legale,tranche_effectifs_unite_legale,etat_administratif_unite_legale,nom_unite_legale,denomination_unite_legale,activite_principale_unite_legale) VALUES (36320596,'1963-01-01',NULL,'3','A',NULL,'BOULANGERIE PATISSERIE L''EPI D''OR','10.71C');
INSERT INTO mytable(siren,date_creation_unite_legale,prenom_unite_legale,tranche_effectifs_unite_legale,etat_administratif_unite_legale,nom_unite_legale,denomination_unite_legale,activite_principale_unite_legale) VALUES (43890441,'1995-12-25',NULL,'1','A',NULL,'COPR 4 RUE DES 2 GARES 75010 PARIS','81.10Z');
