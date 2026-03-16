// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::loaded_data::runtime_types::{StructNameIndex, Type};
use parking_lot::RwLock;
use std::collections::HashMap;
use triomphe::Arc;

#[repr(transparent)]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct TypeId(u32);

#[repr(transparent)]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct TypeVecId(u32);

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
enum TypeRepr {
    Bool,
    U8,
    U16,
    U32,
    U64,
    U128,
    U256,
    Address,
    Signer,
    Vector(TypeId),
    Reference(TypeId),
    MutableReference(TypeId),
    Struct { idx: StructNameIndex, ty_args: TypeVecId },
    TyParam(u16),
}

struct InternMap<T, I> {
    interned: HashMap<T, I>,
    data: Vec<T>,
}

impl<T, I> Default for InternMap<T, I> {
    fn default() -> Self {
        Self {
            interned: HashMap::new(),
            data: Vec::with_capacity(16),
        }
    }
}

impl<T, I> InternMap<T, I> {
    fn clear(&mut self) {
        self.interned.clear();
        self.data.clear();
    }
}

struct TypeInterner {
    inner: RwLock<InternMap<TypeRepr, TypeId>>,
}

impl Default for TypeInterner {
    fn default() -> Self {
        Self {
            inner: RwLock::new(InternMap::default()),
        }
    }
}

impl TypeInterner {
    fn intern(&self, repr: TypeRepr) -> TypeId {
        if let Some(id) = self.inner.read().interned.get(&repr) {
            return *id;
        }

        let mut inner = self.inner.write();
        if let Some(id) = inner.interned.get(&repr) {
            return *id;
        }

        let id = TypeId(inner.data.len() as u32);
        inner.data.push(repr);
        inner.interned.insert(repr, id);
        id
    }
}

struct TypeVecInterner {
    inner: RwLock<InternMap<Arc<[TypeId]>, TypeVecId>>,
}

impl Default for TypeVecInterner {
    fn default() -> Self {
        Self {
            inner: RwLock::new(InternMap::default()),
        }
    }
}

impl TypeVecInterner {
    fn intern(&self, tys: &[TypeId]) -> TypeVecId {
        if let Some(id) = self.inner.read().interned.get(tys) {
            return *id;
        }

        let tys_arced: Arc<[TypeId]> = Arc::from(tys);
        let tys_arced_key = tys_arced.clone();

        let mut inner = self.inner.write();
        if let Some(id) = inner.interned.get(tys) {
            return *id;
        }

        let id = TypeVecId(inner.data.len() as u32);
        inner.data.push(tys_arced);
        inner.interned.insert(tys_arced_key, id);
        id
    }
}

pub struct InternedTypePool {
    ty_interner: TypeInterner,
    ty_vec_interner: TypeVecInterner,
}

impl InternedTypePool {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let ctx = Self {
            ty_interner: TypeInterner::default(),
            ty_vec_interner: TypeVecInterner::default(),
        };
        ctx.warmup();
        ctx
    }

    pub fn num_interned_tys(&self) -> usize {
        self.ty_interner.inner.read().data.len()
    }

    pub fn num_interned_ty_vecs(&self) -> usize {
        self.ty_vec_interner.inner.read().data.len()
    }

    pub fn flush(&self) {
        self.ty_interner.inner.write().clear();
        self.ty_vec_interner.inner.write().clear();
        self.warmup();
    }

    fn warmup(&self) {
        self.ty_interner.intern(TypeRepr::Bool);
        self.ty_interner.intern(TypeRepr::U8);
        self.ty_interner.intern(TypeRepr::U16);
        self.ty_interner.intern(TypeRepr::U32);
        self.ty_interner.intern(TypeRepr::U64);
        self.ty_interner.intern(TypeRepr::U128);
        self.ty_interner.intern(TypeRepr::U256);
        self.ty_interner.intern(TypeRepr::Address);
        self.ty_interner.intern(TypeRepr::Signer);
        self.ty_vec_interner.intern(&[]);
    }

    fn intern_ty_impl(&self, ty: &Type) -> TypeId {
        match ty {
            Type::Bool => self.ty_interner.intern(TypeRepr::Bool),
            Type::U8 => self.ty_interner.intern(TypeRepr::U8),
            Type::U16 => self.ty_interner.intern(TypeRepr::U16),
            Type::U32 => self.ty_interner.intern(TypeRepr::U32),
            Type::U64 => self.ty_interner.intern(TypeRepr::U64),
            Type::U128 => self.ty_interner.intern(TypeRepr::U128),
            Type::U256 => self.ty_interner.intern(TypeRepr::U256),
            Type::Address => self.ty_interner.intern(TypeRepr::Address),
            Type::Signer => self.ty_interner.intern(TypeRepr::Signer),
            Type::Vector(inner) => {
                let inner = self.intern_ty_impl(inner);
                self.ty_interner.intern(TypeRepr::Vector(inner))
            },
            Type::Reference(inner) => {
                let inner = self.intern_ty_impl(inner);
                self.ty_interner.intern(TypeRepr::Reference(inner))
            },
            Type::MutableReference(inner) => {
                let inner = self.intern_ty_impl(inner);
                self.ty_interner
                    .intern(TypeRepr::MutableReference(inner))
            },
            Type::Struct { idx, .. } => self.ty_interner.intern(TypeRepr::Struct {
                idx: *idx,
                ty_args: self.ty_vec_interner.intern(&[]),
            }),
            Type::StructInstantiation { idx, ty_args, .. } => {
                let ty_args = self.intern_ty_args(ty_args);
                self.ty_interner.intern(TypeRepr::Struct {
                    idx: *idx,
                    ty_args,
                })
            },
            Type::TyParam(idx) => self.ty_interner.intern(TypeRepr::TyParam(*idx)),
        }
    }

    pub fn intern_ty(&self, ty: &Type) -> TypeId {
        self.intern_ty_impl(ty)
    }

    pub fn intern_ty_args(&self, tys: &[Type]) -> TypeVecId {
        let ty_ids = tys.iter().map(|ty| self.intern_ty_impl(ty)).collect::<Vec<_>>();
        self.ty_vec_interner.intern(&ty_ids)
    }
}
