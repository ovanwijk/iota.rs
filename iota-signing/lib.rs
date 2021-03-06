#![warn(
    missing_debug_implementations,
    missing_docs,
    rust_2018_idioms,
    unreachable_pub
)]

//! Methods facilitating signing for Iota

pub use hmac::HMAC;
use iota_constants;
use iota_constants::HASH_TRINARY_SIZE;
use iota_conversion::Trinary;
use iota_crypto::{Kerl, Sponge};
use iota_model::Bundle;
use iota_validation::input_validator;

/// Checksum functions and utilities
pub mod checksum;
mod hmac;

type Result<T> = ::std::result::Result<T, failure::Error>;

const KEY_LENGTH: usize = 6561;

/// Key
pub fn key(in_seed: &[i8], index: usize, security: usize) -> Result<Vec<i8>> {
    if security < 1 {
        panic!(iota_constants::INVALID_SECURITY_LEVEL_INPUT_ERROR);
    }
    let mut seed = in_seed.to_owned();
    for _i in 0..index {
        for trit in &mut seed {
            *trit += 1;
            if *trit > 1 {
                *trit = -1;
            } else {
                break;
            }
        }
    }
    let mut curl = Kerl::default();
    curl.reset();
    curl.absorb(&seed)?;
    curl.squeeze(&mut seed)?;
    curl.reset();
    curl.absorb(&seed)?;

    let mut key = vec![0; security * HASH_TRINARY_SIZE * 27];
    let mut buffer = vec![0; seed.len()];
    let mut offset = 0;

    let mut tmp_sec = security;
    while tmp_sec > 0 {
        for _i in 0..27 {
            curl.squeeze(&mut buffer)?;
            key[offset..offset + HASH_TRINARY_SIZE].copy_from_slice(&buffer[0..HASH_TRINARY_SIZE]);
            offset += HASH_TRINARY_SIZE;
        }
        tmp_sec -= 1;
    }
    Ok(key)
}

/// Signs a signature fragment
pub fn signature_fragment(
    normalized_bundle_fragment: &[i8],
    key_fragment: &[i8],
) -> Result<Vec<i8>> {
    let mut signature_fragment = key_fragment.to_owned();
    let mut curl = Kerl::default();
    for (i, fragment) in normalized_bundle_fragment.iter().enumerate().take(27) {
        let mut j = 0;
        while j < 13 - fragment {
            curl.reset();
            let offset = i * HASH_TRINARY_SIZE;
            curl.absorb(&signature_fragment[offset..offset + HASH_TRINARY_SIZE])?;
            curl.squeeze(&mut signature_fragment[offset..offset + HASH_TRINARY_SIZE])?;
            j += 1;
        }
    }
    Ok(signature_fragment)
}

/// Signs an address
pub fn address(digests: &[i8]) -> Result<[i8; HASH_TRINARY_SIZE]> {
    let mut address = [0; HASH_TRINARY_SIZE];
    let mut curl = Kerl::default();
    curl.reset();
    curl.absorb(digests)?;
    curl.squeeze(&mut address)?;
    Ok(address)
}

/// Signs digests
pub fn digests(key: &[i8]) -> Result<Vec<i8>> {
    let security = (key.len() as f64 / KEY_LENGTH as f64).floor() as usize;
    let mut digests = vec![0; security * HASH_TRINARY_SIZE];
    let mut key_fragment = [0; KEY_LENGTH];
    let mut curl = Kerl::default();
    for i in 0..security {
        let offset = i * KEY_LENGTH;
        key_fragment[0..KEY_LENGTH].copy_from_slice(&key[offset..offset + KEY_LENGTH]);
        for j in 0..27 {
            for _k in 0..26 {
                curl.reset();
                let offset = j * HASH_TRINARY_SIZE;
                curl.absorb(&key_fragment[offset..offset + HASH_TRINARY_SIZE])?;
                curl.squeeze(&mut key_fragment[offset..offset + HASH_TRINARY_SIZE])?;
            }
        }
        curl.reset();
        curl.absorb(&key_fragment)?;
        let offset = i * HASH_TRINARY_SIZE;
        curl.squeeze(&mut digests[offset..offset + HASH_TRINARY_SIZE])?;
    }
    Ok(digests)
}

/// Signs a digest
pub fn digest(normalized_bundle_fragment: &[i8], signature_fragment: &[i8]) -> Result<Vec<i8>> {
    let mut curl = Kerl::default();
    curl.reset();
    let mut j_curl = Kerl::default();
    let mut buffer = vec![0; HASH_TRINARY_SIZE];
    for i in 0..27 {
        buffer = signature_fragment[i * HASH_TRINARY_SIZE..(i + 1) * HASH_TRINARY_SIZE].to_vec();
        let mut j = normalized_bundle_fragment[i] + 13;
        while j > 0 {
            j_curl.reset();
            j_curl.absorb(&buffer)?;
            j_curl.squeeze(&mut buffer)?;
            j -= 1;
        }
        curl.absorb(&buffer)?;
    }
    curl.squeeze(&mut buffer)?;
    Ok(buffer)
}

/// Validates signatures for a bundle
pub fn validate_bundle_signatures(signed_bundle: &Bundle, address: &str) -> Result<bool> {
    let mut bundle_hash = "";
    let mut signature_fragments: Vec<String> = Vec::new();
    for transaction in signed_bundle.iter() {
        if transaction.address == address {
            bundle_hash = &transaction.bundle;
            let signature_fragment = &transaction.signature_fragments;
            if input_validator::is_nine_trytes(&signature_fragment) {
                break;
            }
            signature_fragments.push(signature_fragment.clone());
        }
    }
    validate_signatures(address, &signature_fragments, &bundle_hash)
}

/// Validates signatures
pub fn validate_signatures(
    expected_address: &str,
    signature_fragments: &[String],
    bundle_hash: &str,
) -> Result<bool> {
    let mut normalized_bundle_fragments = [[0; 27]; 3];
    let normalized_bundle_hash = Bundle::normalized_bundle(bundle_hash);

    for i in 0..3 {
        normalized_bundle_fragments[i]
            .copy_from_slice(&normalized_bundle_hash[i * 27..(i + 1) * 27]);
    }
    let mut digests = vec![0; signature_fragments.len() * HASH_TRINARY_SIZE];

    for i in 0..signature_fragments.len() {
        let digest_buffer = digest(
            &normalized_bundle_fragments[i % 3],
            &signature_fragments[i].trits(),
        )?;
        let offset = i * HASH_TRINARY_SIZE;
        digests[offset..offset + HASH_TRINARY_SIZE]
            .copy_from_slice(&digest_buffer[0..HASH_TRINARY_SIZE]);
    }
    let address = address(&digests)?.trytes()?;
    Ok(expected_address == address)
}

#[cfg(test)]
mod tests {
    use super::*;
    use checksum::remove_checksum;
    use iota_model::Bundle;

    const TEST_SEED: &str =
        "IHDEENZYITYVYSPKAURUZAQKGVJEREFDJMYTANNXXGPZ9GJWTEOJJ9IPMXOGZNQLSNMFDSQOTZAEETUEA";
    const FIRST_ADDR: &str = "LXQHWNY9CQOHPNMKFJFIJHGEPAENAOVFRDIBF99PPHDTWJDCGHLYETXT9NPUVSNKT9XDTDYNJKJCPQMZCCOZVXMTXC";
    const ADDR: &str = "HLHRSJNPUUGRYOVYPSTEQJKETXNXDIWQURLTYDBJADGIYZCFXZTTFSOCECPPPPY9BYWPODZOCWJKXEWXDPUYEOTFQA";
    const SIG1: &str = "PYWFM9MYTPNZ9HTLZBBB9CGQWKPALDUNAQYCAA9VMQ9UMBLLAXSPPHQSNAAKJA9MZBXBHBQBFFKMBSDHDTCVCDWLUYCEQ9YZJAJAXXXZHDWTSLWGIWRE9LJFVWAFUMOAGHDBHJQ9APNBLSX9GPTJNTO9SBJT9UKYCZXYAWVGXEBJANNWEWZSPRYHASHGIFUWOEHUFMP9MWQBYZOZESCPLVJUCWGLEJIDPMEVNPBITBNFSQ9GBWCDTQZOPLPXOWWNQAEIXQRWMHAQDH9C9KKHGNKAX9INMUVVGIK9TPGRHOMDFAB9VICYDMSHHDDBRSTEFSZXMXFJUQRRAFBSCNHSMKRNNTTCMBURKBGC9EDWKLPBSQAKYCUKKSZWRVURZGUA9QVSXXPICIYFHLPJSWEFBZPUTWWNIKSAJM9OMRFFQVFJZZHLQBSEYXM9CN9HCGHSJBTYDGWOQPXOPZZE9EPQAQFT9GDWZCSOPMZHYYZXDDZ9DJDLOOOTIFQANFANNAYVIRUNDXSB9XRNXJYRDBLTEDWSUOVISMCHGKD9KDRSFDWRSVZQQKGAMDXFAWBSLMTTUMH9RAUIVI9HJMTODACSOP9MLHOJMSIWQ9TTNGPXRNWRHLMEMAH9GZHJRNJHQNBBLWKFXIZBMGMATZIZBFDPAFDCLDIFFAIK9JUSFYYC9ANDGXCZFLZYGURTUI9SWYYRGDJAHXDDNHSJZBCENZUSQXSFZMTXSFLRK9RIYAUMHPBOBNOXCHDIMBGIBVOOHIDQ9ORHHDECDTREIEILWDUFMUWYMGIXBIKRZMKGXTYZTX9GKFP9AUXMTUUQXRHHKPYULGJFJLEEYCNKLOWULRIAFM9OYKEDFRXFVTSJMSEMOURCLNOIETIHEUCMPLWKDXDO9TAHVH99MKTBAAKCMYKLJUQIVLLSVTFUM9KDSIHYXYHPRLDADSLSSOIGLLXMPKTHS9YXUNMUTBTBPDWXA9GVTBGLTCLEZEUNNIRBBURDWOFFYXELPFSZRQARVRPHGETKJTRUZIFDDWBOHHGUZTODZFMOVMAGCYCTGBWSGAVZADIPIASCKTRKIUUMHNGUYZKDVOPKKHXD9EXVUVJ9YFNYMLIJLEEGPIZLFS9FIEMG9MIEO9FPW9JZEVDQOECMTESICSMVWXZNXXJILJLVQHEBHQWPOBHKEGRLFCPLB9ZECJOZDAB9DMU9UALBIQDABVDYRRTPMZOCQX9WNGXVNKQZWPA9ACVONQMRHQDPPIQTP9VKP9PAORNOFTZZWGC9RYBWSNLULZGYLMYIWWPDMOHPZTQWRPRCN9RAUOKDSCWBRI9NPUPLBILOZDOOPHSWQGJEGUYWAWJDEBLEOBSYYU9XSRPBHRUQXIDOWJZQQVJTMP9VLWLOGBK9FZFHYLJCNENDATNPSF99DFPVPTNNKIUMHRGEBJXNUVENAHYLFPPHYFTIKCB9DBVCCSJTDMOMISBAAEJVBVLHOADKNFG9NQGIGRDICQCWZVHGGXLTUNQKBUTLDWXIM9REWBLIXFBPTOXBLWBQQUSRLRDHTXQWARPMBQILAJSYLLTDAGTFPCXBCDITDOIZNGKPZQWWHJDZIPYCPFEYFD9CVXYOJHJNUNMCMSIAUVSKCACNNPGDYJJVTZOREJOPIBYCMBULMTSDTJPZNVNYQBQPPABOSSNZJKQQZ9LULSHJUBLHIFMYWSNPGUERCLVFV9LOEBJEERYHI9OMSMSCDFDLNHEMLQXNRJDYSNKTOYCPTAUWAWIGCPJKMAMGLXNBJMO9BZGFIHWDVJWYCNZZV9KBWIFQSMAXBPGVXDW9SLTHOLMJORRXZJSTNOQDRGNBLGTFCCNBJECYZGWTDRJKJRBAJRCULMOUBQJFWCLWMEWGAAVNZWMDWBYDKZMUCZAKXQLRQPIQJPMORKJXKSDTGXWDHAKUOSMXCFXWSZYWXODWFACBMFSWQFVMBELPZMISVWRQQQPNHOTWOEQQAQJDLXFEEBXLJQEECWG9ARRRDLTVBHTPARJMLOZHYWDCSXPTZCNZWTCRUJNZWKFZXAARPHFCBTLWSLERGJJMKIG9NEBADRMZWYNWIRGTMOBRKURUE9GDLRIEODY9BXJOZUVNCXKXFPFDXKUTMXZRJDOQ9YTV9BJDKGZBYTWGVPQQMNVCNARLPSRQWN9TRMHWLNEJZFTCSRD";
    const SIG2: &str = "URKFKLNXFEKDOGSQVMAOPEDIWSMTCKJZ9KEVWYALY9JAO9KHUGNDTMGQLKQJUIPWDIVMPEDSVPLFMDCIXDDT9WBBRTFQENL9AXLSBYHINXCDYBFGRNKJDYHAQVJKWCVOYXHTNBEZUNLVMJLUMZYJFAOW9PVVMJZNZZFJQEQFELVFZVFVWPJ9WQZJLPSGBYECHXSFVFQJGUCPFXC9GATTILVCAANNHOYMLOYX9QSUPCERYCOXPACZEEGLREBRZWXGUTTVTHB9GBRCIFEOBPIRXXPQKRSODEHDSZXLGIKXUQWNTQKIOPVDVSIK9WJUAEFOJBU9MBPBSVYSCLBMINTT9ZCTREZSMSVOPXSZOMCGFEZKMOCNLJ9QUTAPKBHRIAIYLCHUQHOINKSCMXWZVDGDXHNJQXJHPCCGBEWROVKEPAPBFFRCAVXZWIRKCRAWYHIHMDXFAGDJQNJJPYSQUHKFOOCEVQOGRQEIOQFKZWUQ9XVRNXKGMJOQEZHQZXQABWUQRBKXWHYUXEAEMDGXVY9WS9VJOCMGBQASSRNKAYJPTSPQEMYSJMTCLMDQJKDPBGQZZSFBDOKHBYY9UDRXNKTPWBCQTVKUGMEDUXL9TTKPATNIKVAGHACHPFSCRYNIRJBQC9OADPGWBFYYARSVNQCGMYQGCYLZH9KLMUIJPCLPQVS9BORXCJBXPDECJGKDNOUYWTKKFLXZARWKGUSMVMXKJTMRYZRERFCFGTZFZFCAOQSZGPQJUEZUJLJPU9QPMJUTZNLMSMPRGIFHUUZHMPMRBEBATEIIWPCOIMWOYOG9NYFBYOWFDKRXOTREBU99GNCPXKOWGI99LNVPRFFF9FCLFXI9HMUFU9NRLNJVTFNUSUJTAVOG9GKUYYEXIM9HTPIDTWIGLKRAQPKMQVZAPYMPSQIOJ9JZBWDMQHDSSRSHNCWSAJCSRORSEXLLQNZUKPXPGRLYMXOXWCCWWSBALFLXPHSGFLTOAFWPETBKJUMBLHMSKYLPJT9EJAZCPPNZWKPVCGKDJCRCLBBIAKVDSNWGONPLKFAYXZDI9FKPHDPKCB9UUPXLJVQTXOAZOQDRNSONXDVSLQGZYRIPGREYHRAUOSBFZDZPZHFNMWCZQGPXCZVLNCSASB9RQDFHOYMUVYLFKOEEWNREYCDMCTZIAFBFKLKRQWZCJHQZCZGWXIFTKRVMPHMVHAABHBDEV9WDEZBR9FLXLNBVNYKUOUFJQKNZVZVGZDDTFYNYFUVRLZKOLXXQYNV9MDVBLZSERXPGYKRIEZQZD9IBKFDT9AIYGWJJCXFWDUDURGJQLXVEJAVEOMZUVVTNCVBXEVQRDQIEHDUCSLCIJUTSCLFXEGMFYP9YLXELCZPMTBZWBIODZCFNJLVWTPQGLMQIHIABAYGJFFMOEDTCXGEDTNXMVXZYFGXRKVVRTIZ9ISXTDHAFPEKQZSM9XXQLOYBLTMD9MBERBIBEJDEXGMOLDZPZVVEPIRKJBDPAKFAWJPTCJSHZPDUKZEEHRFLMZCUGCOWFJBSTDGPHUIXSPPPHRQARMCFMTWKYPJNJQV9VSFZ9EWB9GVEAFUXHWRNUXQLCSBWROOITBATWUXUYGSMGAXKGEBP9ZJWXQWHBVPOSLDHTWXUOFQNO9EXSYPQF9LQLQAFNRU9MTIIRQLBBBYKUPANWRQKGESFARQIRUTGFMZVUKHZJYKTYOARTDOBIYBFRHJWEFHCYVHRHTLTWBRMUDVIVQVNELQMQRXYDNGVSICZINWIZCIWVFXLYOLYKWDNWCWFZUXHUWOPRDHMTSXOZX9CVHANU9ZXTJOGKEPYR9CHGOTIUQSWIALAOIKHQFXWY9ZWTSZADVXJNNZOLSCXVVFBRHLRBTGMSZOYNIXTAMABKGJTLGTZKRHOPPJMNYIQNVKRGXUQDWYEIEZYM9CSXO9YLSBJLDJUWOLUXDEKBGGEIDEXFLZMESDOITNYTNRLGOMHJH9HOLXJABUNLXCZYTXFPZMHRJPLXSVPDBJBBZX9TBIMZZFZOXUSFEJYHEXPFXGJCQTBBLPEEWAPHUETGXSXYYAF9PCCCOONRMQGAPJ9JO9BZQ9QSKTPFFYIFVHSLAZY9CWYSIMKDOSLRKWBHPGJGVEJEEMLCCWXKSOCMBMZZZJWYBBXE9FTAYJALGWITJRXAXWZEXMECTZEEIWZPHYX";

    #[test]
    fn test_long_seed_key_generation() {
        let seed =
            "EV9QRJFJZVFNLYUFXWKXMCRRPNAZYQVEYB9VEPUHQNXJCWKZFVUCTQJFCUAMXAHMMIUQUJDG9UGGQBPIY";

        for i in 1..5 {
            let key1 = key(&seed.trits(), 0, i).unwrap();
            assert_eq!(KEY_LENGTH * i, key1.len());
            let key2 = key(&(seed.to_string() + seed).trits(), 0, i).unwrap();
            assert_eq!(KEY_LENGTH * i, key2.len());
            let key3 = key(&(seed.to_string() + seed + seed).trits(), 0, i).unwrap();
            assert_eq!(KEY_LENGTH * i, key3.len());
        }
    }

    #[test]
    fn test_signing() {
        let hash_to_sign = remove_checksum("LXQHWNY9CQOHPNMKFJFIJHGEPAENAOVFRDIBF99PPHDTWJDCGHLYETXT9NPUVSNKT9XDTDYNJKJCPQMZCCOZVXMTXC");
        let key = key(&TEST_SEED.trits(), 5, 2).unwrap();
        let normalized_hash = Bundle::normalized_bundle(&hash_to_sign);
        let signature = signature_fragment(&normalized_hash[0..27], &key[0..6561]).unwrap();
        assert_eq!(signature.trytes().unwrap(), SIG1);
        let signature2 =
            signature_fragment(&normalized_hash[27..27 * 2], &key[6561..6561 * 2]).unwrap();
        assert_eq!(signature2.trytes().unwrap(), SIG2);
    }

    #[test]
    fn test_key_length() {
        let mut test_key = key(&TEST_SEED.trits(), 5, 1).unwrap();
        assert_eq!(KEY_LENGTH, test_key.len());
        test_key = key(&TEST_SEED.trits(), 5, 2).unwrap();
        assert_eq!(KEY_LENGTH * 2, test_key.len());
        test_key = key(&TEST_SEED.trits(), 5, 3).unwrap();
        assert_eq!(KEY_LENGTH * 3, test_key.len());
    }

    #[test]
    fn test_verifying() {
        assert!(validate_signatures(
            &remove_checksum(ADDR),
            &vec![SIG1.to_string(), SIG2.to_string()],
            &remove_checksum(FIRST_ADDR),
        )
        .unwrap());
    }
}
