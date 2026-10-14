//! Conversions entre le stockage SQLite et le format JSON de l'API.

use std::time::{SystemTime, UNIX_EPOCH};

/// Nombre de chiffres d'un SIREN.
const LONGUEUR_SIREN: usize = 9;

/// Lit un SIREN saisi dans l'URL : uniquement des chiffres, 9 au plus (les zéros de tête peuvent manquer).
pub fn parse_siren(brut: &str) -> Option<i64> {
    if brut.is_empty() || brut.len() > LONGUEUR_SIREN || !brut.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    brut.parse().ok()
}

/// Restitue les zéros de tête perdus dans le fichier : `325175` → `"000325175"`.
pub fn format_siren(siren: i64) -> String {
    format!("{siren:09}")
}

/// Complète la tranche d'effectifs sur 2 caractères : `"3"` → `"03"`, `"NN"` inchangé.
pub fn pad_tranche(brut: &str) -> String {
    let tranche = brut.trim();
    if tranche.len() == 1 && tranche.as_bytes()[0].is_ascii_digit() {
        format!("0{tranche}")
    } else {
        tranche.to_owned()
    }
}

/// `A` → `active`, `C` → `cessee`.
pub fn etat_vers_api(code: &str) -> &'static str {
    if code == "A" { "active" } else { "cessee" }
}

/// `active` → `A`, `cessee` → `C`.
pub fn etat_depuis_api(valeur: &str) -> Option<&'static str> {
    match valeur {
        "active" => Some("A"),
        "cessee" => Some("C"),
        _ => None,
    }
}

/// Texte de recherche : minuscules, sans accents (`deunicode`), sans caractères de contrôle,
/// espaces multiples réduits à un seul.
///
/// Le `°` (« N°26 ») est gardé tel quel : c'est le seul caractère non ASCII du fichier Sirene.
pub fn fold(texte: &str) -> String {
    let mut sortie = String::with_capacity(texte.len());
    for c in texte.chars() {
        if c.is_ascii() || c == '°' {
            ajouter(&mut sortie, c);
        } else if let Some(ascii) = deunicode::deunicode_char(c) {
            // é → e, Œ → OE, ’ → ', accent combinant → rien.
            ascii.chars().for_each(|a| ajouter(&mut sortie, a));
        }
    }
    if sortie.ends_with(' ') {
        sortie.pop();
    }
    sortie
}

fn ajouter(sortie: &mut String, c: char) {
    if c.is_whitespace() {
        if !sortie.is_empty() && !sortie.ends_with(' ') {
            sortie.push(' ');
        }
    } else if !c.is_control() {
        sortie.extend(c.to_lowercase());
    }
}

/// Colonne `recherche` : dénomination, nom et prénom réunis puis normalisés par [`fold`].
pub fn texte_recherche(denomination: Option<&str>, nom: Option<&str>, prenom: Option<&str>) -> String {
    let morceaux: Vec<&str> = [denomination, nom, prenom].into_iter().flatten().collect();
    fold(&morceaux.join(" "))
}

/// Échappe `%`, `_` et `\` pour un `LIKE ... ESCAPE '\'`.
pub fn echapper_like(texte: &str) -> String {
    let mut sortie = String::with_capacity(texte.len());
    for c in texte.chars() {
        if matches!(c, '%' | '_' | '\\') {
            sortie.push('\\');
        }
        sortie.push(c);
    }
    sortie
}

/// Vérifie une date `AAAA-MM-JJ` : date du calendrier, pas dans le futur.
pub fn verifier_date(texte: &str) -> Result<(), &'static str> {
    let octets = texte.as_bytes();
    let format_ok = octets.len() == 10
        && octets[4] == b'-'
        && octets[7] == b'-'
        && octets.iter().enumerate().all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit());
    if !format_ok {
        return Err("Format attendu : AAAA-MM-JJ.");
    }
    let annee: i64 = texte[0..4].parse().map_err(|_| "Année invalide.")?;
    let mois: u32 = texte[5..7].parse().map_err(|_| "Mois invalide.")?;
    let jour: u32 = texte[8..10].parse().map_err(|_| "Jour invalide.")?;
    if !(1..=12).contains(&mois) || jour == 0 || jour > jours_dans_mois(annee, mois) {
        return Err("Cette date n'existe pas.");
    }
    if (annee, mois, jour) > aujourdhui() {
        return Err("La date ne peut pas être dans le futur.");
    }
    Ok(())
}

fn jours_dans_mois(annee: i64, mois: u32) -> u32 {
    match mois {
        2 if (annee % 4 == 0 && annee % 100 != 0) || annee % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Date du jour (UTC) en (année, mois, jour), sans dépendance à une crate de dates.
fn aujourdhui() -> (i64, u32, u32) {
    let secondes = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    date_civile((secondes / 86_400) as i64)
}

/// Jours depuis le 1970-01-01 → date du calendrier grégorien (algorithme de H. Hinnant).
fn date_civile(jours: i64) -> (i64, u32, u32) {
    let z = jours + 719_468;
    let ere = z.div_euclid(146_097);
    let jour_ere = z.rem_euclid(146_097);
    let annee_ere = (jour_ere - jour_ere / 1460 + jour_ere / 36_524 - jour_ere / 146_096) / 365;
    let jour_annee = jour_ere - (365 * annee_ere + annee_ere / 4 - annee_ere / 100);
    let mp = (5 * jour_annee + 2) / 153;
    let jour = (jour_annee - (153 * mp + 2) / 5 + 1) as u32;
    let mois = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let annee = annee_ere + ere * 400 + i64::from(mois <= 2);
    (annee, mois, jour)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn siren_accepte_les_zeros_omis() {
        assert_eq!(parse_siren("325175"), Some(325_175));
        assert_eq!(parse_siren("000325175"), Some(325_175));
        assert_eq!(parse_siren("036320596"), Some(36_320_596));
    }

    #[test]
    fn siren_refuse_non_numerique_ou_trop_long() {
        for brut in ["", "abc", "12a", "-12", "+12", " 12", "1234567890", "0000000001"] {
            assert_eq!(parse_siren(brut), None, "{brut:?}");
        }
    }

    #[test]
    fn siren_formate_sur_9_chiffres() {
        assert_eq!(format_siren(325_175), "000325175");
        assert_eq!(format_siren(36_320_596), "036320596");
        assert_eq!(format_siren(43_890_607), "043890607");
    }

    #[test]
    fn tranche_sur_2_caracteres() {
        assert_eq!(pad_tranche("3"), "03");
        assert_eq!(pad_tranche("0"), "00");
        assert_eq!(pad_tranche("21"), "21");
        assert_eq!(pad_tranche("NN"), "NN");
    }

    #[test]
    fn etat_administratif() {
        assert_eq!(etat_vers_api("A"), "active");
        assert_eq!(etat_vers_api("C"), "cessee");
        assert_eq!(etat_depuis_api("active"), Some("A"));
        assert_eq!(etat_depuis_api("cessee"), Some("C"));
        assert_eq!(etat_depuis_api("Active"), None);
    }

    #[test]
    fn fold_retire_casse_accents_et_espaces() {
        assert_eq!(fold("Boulangérie"), "boulangerie");
        assert_eq!(fold("  ÉCOLE   Œuvre  Garçon "), "ecole oeuvre garcon");
        assert_eq!(fold("Boulange\u{301}rie"), "boulangerie");
        assert_eq!(fold("COPROPRIETE JOUVENEL N°26"), "copropriete jouvenel n°26");
        assert_eq!(fold("boulang\u{1ebd}rie BǑULANGẾRIE boulangeriẹ"), "boulangerie boulangerie boulangerie");
        assert_eq!(fold("L’ÉPI D’OR"), "l'epi d'or");
        assert_eq!(fold("Straße ÆTNA"), "strasse aetna");
    }

    #[test]
    fn fold_retire_les_caracteres_de_controle() {
        assert_eq!(fold("\0\0\0"), "");
        assert_eq!(fold("a\u{0}b\u{7f}c\td"), "abc d");
    }

    #[test]
    fn recherche_reunit_les_trois_champs() {
        assert_eq!(texte_recherche(None, Some("JANOYER"), Some("THIERRY")), "janoyer thierry");
        assert_eq!(texte_recherche(Some("L'ÉPI D'OR"), None, None), "l'epi d'or");
        assert_eq!(texte_recherche(None, None, None), "");
    }

    #[test]
    fn like_echappe_les_jokers() {
        assert_eq!(echapper_like("50%_a\\b"), "50\\%\\_a\\\\b");
    }

    #[test]
    fn dates() {
        assert!(verifier_date("1963-01-01").is_ok());
        assert!(verifier_date("2024-02-29").is_ok());
        assert!(verifier_date("2023-02-29").is_err());
        assert!(verifier_date("2023-13-01").is_err());
        assert!(verifier_date("1963-1-1").is_err());
        assert!(verifier_date("9999-01-01").is_err());
    }

    #[test]
    fn date_civile_connue() {
        assert_eq!(date_civile(0), (1970, 1, 1));
        assert_eq!(date_civile(20_719), (2026, 9, 23));
    }
}
