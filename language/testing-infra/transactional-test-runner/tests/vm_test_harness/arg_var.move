//# init --addresses A=42

//# publish
module A::M {
    struct Foo has key {
        x: u64,
    }

    public fun foo(s: &signer): u64 acquires Foo {
        borrow_global<Foo>(std::signer::address_of(s)).x
    }

    public fun publish_foo(s: &signer) {
        move_to<Foo>(s, Foo { x: 500 })
    }
}



//# run --signers A
script {
    use A::M;

    fun main(s: signer) {
        M::publish_foo(&s);
    }
}


//# view --address A --resource 0x2a::M::Foo

//# run --signers A
script {
    use A::M;

    fun main(s: signer) {
        assert!(M::foo(&s) == 500, 1001);
    }
}