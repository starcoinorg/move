//# init --addresses A=0x42

//# publish
module A::M {
    use std::signer;
    struct SignerCapability has store, drop {
        addr: address
    }

    fun init_signer_cap(addr: address): SignerCapability {
        let signer_cap = SignerCapability { addr };
        signer_cap
    }

     fun create_signer_with_cap(cap: &SignerCapability) : signer {
        signer::signer_create(cap.addr)
    }

    public entry fun test(s: signer) {
        let signer_cap = init_signer_cap(std::signer::address_of(&s));
        let _s = create_signer_with_cap(&signer_cap);
    }
}

//# run --signers 0x1 -- 0x42::M::test