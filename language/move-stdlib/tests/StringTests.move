#[test_only]
module Std::StringTests {
    use Std::String;

    #[test]
    fun test_valid_utf8() {
        let sparkle_heart = vector[240, 159, 146, 150];
        let s = String::utf8(sparkle_heart);
        assert!(String::length(&s) == 4, 22);
    }

    #[test]
    #[expected_failure(abort_code = 1)]
    fun test_invalid_utf8() {
        let no_sparkle_heart = vector[0, 159, 146, 150];
        let s = String::utf8(no_sparkle_heart);
        assert!(String::length(&s) == 1, 22);
    }

    #[test]
    fun test_sub_string() {
        let s = String::utf8(b"abcd");
        let sub = String::sub_string(&s, 2, 4);
        assert!(sub == String::utf8(b"cd"), 22)
    }

    #[test]
    #[expected_failure(abort_code = 2)]
    fun test_sub_string_invalid_boundary() {
        let sparkle_heart = vector[240, 159, 146, 150];
        let s = String::utf8(sparkle_heart);
        let _sub = String::sub_string(&s, 1, 4);
    }

    #[test]
    #[expected_failure(abort_code = 2)]
    fun test_sub_string_invalid_index() {
        let s = String::utf8(b"abcd");
        let _sub = String::sub_string(&s, 4, 5);
    }

    #[test]
    fun test_sub_string_empty() {
        let s = String::utf8(b"abcd");
        let sub = String::sub_string(&s, 4, 4);
        assert!(String::is_empty(&sub), 22)
    }

    #[test]
    fun test_index_of() {
        let s = String::utf8(b"abcd");
        let r = String::utf8(b"bc");
        let p = String::index_of(&s, &r);
        assert!(p == 1, 22)
    }

    #[test]
    fun test_index_of_fail() {
        let s = String::utf8(b"abcd");
        let r = String::utf8(b"bce");
        let p = String::index_of(&s, &r);
        assert!(p == 4, 22)
    }

    #[test]
    fun test_append() {
        let s = String::utf8(b"abcd");
        String::append(&mut s, String::utf8(b"ef"));
        assert!(s == String::utf8(b"abcdef"), 22)
    }

    #[test]
    fun test_insert() {
        let s = String::utf8(b"abcd");
        String::insert(&mut s, 1, String::utf8(b"xy"));
        assert!(s == String::utf8(b"axybcd"), 22)
    }
}