use ccez_keys_core::telex::telex;

#[test]
fn vietnamese_words() {
    for (typed, want) in [
        ("Vieetj", "Việt"), ("Nam", "Nam"), ("dduowcj", "được"), ("tieengs", "tiếng"), ("chaof", "chào"),
        ("khoong", "không"), ("cuar", "của"), ("nguowif", "người"), ("hoaf", "hòa"), ("toans", "toán"),
        ("quas", "quá"), ("gias", "giá"), ("muaf", "mùa"), ("ddi", "đi"), ("mawtj", "mặt"), ("ngoaif", "ngoài"),
    ] {
        assert_eq!(telex(typed), want, "{typed}");
    }
}

#[test]
fn a_tone_key_twice_gives_the_letter() {
    assert_eq!(telex("ass"), "as");
}
