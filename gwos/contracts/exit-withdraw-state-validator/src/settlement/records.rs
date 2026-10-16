//! Fixed-layout Molecule records; exact decoding rejects unknown tags and trailing bytes.
use crate::{Error, Hash, Result};
use alloc::vec::Vec;
use gw_hash::blake2b::hash;
use gw_types::U256;
pub type Eth = [u8; 20];
pub const INITIAL: u8 = 0;
pub const DEFERRED: u8 = 1;
pub const EXCLUDED: u8 = 2;
pub const PROTECTED: u8 = 3;
pub const PENDING: u8 = 0;
pub const ACTIVE: u8 = 1;

pub struct Cursor<'a>(pub &'a [u8]);
impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.0.len() < n {
            return Err(Error::Encoding);
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
}
pub trait Field: Sized {
    const SIZE: usize;
    fn write(&self, out: &mut Vec<u8>);
    fn read(input: &mut Cursor<'_>) -> Result<Self>;
}
impl<const N: usize> Field for [u8; N] {
    const SIZE: usize = N;
    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self);
    }
    fn read(c: &mut Cursor<'_>) -> Result<Self> {
        Ok(c.take(N)?.try_into().map_err(|_| Error::Encoding)?)
    }
}
macro_rules! integer {
    ($t:ty,$n:expr) => {
        impl Field for $t {
            const SIZE: usize = $n;
            fn write(&self, o: &mut Vec<u8>) {
                o.extend_from_slice(&self.to_le_bytes());
            }
            fn read(c: &mut Cursor<'_>) -> Result<Self> {
                Ok(<$t>::from_le_bytes(
                    c.take($n)?.try_into().map_err(|_| Error::Encoding)?,
                ))
            }
        }
    };
}
integer!(u8, 1);
integer!(u32, 4);
integer!(u64, 8);
impl Field for U256 {
    const SIZE: usize = 32;
    fn write(&self, o: &mut Vec<u8>) {
        let mut b = [0; 32];
        self.to_little_endian(&mut b);
        o.extend_from_slice(&b);
    }
    fn read(c: &mut Cursor<'_>) -> Result<Self> {
        Ok(U256::from_little_endian(c.take(32)?))
    }
}
pub fn encode<T: Field>(v: &T) -> Vec<u8> {
    let mut out = Vec::with_capacity(T::SIZE);
    v.write(&mut out);
    out
}
pub fn decode<T: Field>(b: &[u8]) -> Result<T> {
    let mut c = Cursor(b);
    let v = T::read(&mut c)?;
    if !c.0.is_empty() {
        return Err(Error::Encoding);
    }
    Ok(v)
}
macro_rules! record {($n:ident,$tag:expr,{$($f:ident:$t:ty),*})=>{
    #[derive(Clone,Copy,Debug,PartialEq,Eq,Default)] pub struct $n {$(pub $f:$t),*}
    impl Field for $n {
        const SIZE:usize=4 $(+ <$t as Field>::SIZE)*;
        fn write(&self,o:&mut Vec<u8>){($tag as u32).write(o);$(self.$f.write(o);)*}
        fn read(c:&mut Cursor<'_>)->Result<Self>{if u32::read(c)?!=$tag{return Err(Error::Encoding)}Ok(Self{$($f:<$t>::read(c)?),*})}
    }
};}
record!(Source, 512, {account:u32, script_hash:Hash, address:Eth, asset:u32, type_hash:Hash, baseline:U256, evidence:Hash});
record!(Lot, 513, {id:Hash, source:Hash, kind:u8, amount:U256, evidence:Hash, reopen:Hash});
record!(Reserve, 514, {remaining:U256});
record!(Claim, 515, {epoch:u64, source:Hash, lot:Hash, recipient:u32, script_hash:Hash, address:Eth, cap:U256, evidence:Hash});
record!(SourceBudget, 516, {paid:U256});
record!(AssetLimit, 517, {asset:u32, type_hash:Hash, ceiling:U256});
record!(AssetBudget, 518, {paid:U256});
record!(Epoch, 519, {id:u64, claim_count:u64, manifest:Hash, policy:Hash, status:u8});
record!(Paid, 520, {amount:U256});
record!(Payment, 522, {claim:Claim, amount:U256});
record!(Metadata, 523, {deployment:Hash, profile:Hash, snapshot:Hash, sequence:u64, state_root:Hash});

pub fn commitment(bytes: &[u8]) -> Hash {
    if bytes.is_empty() {
        return [0; 32];
    }
    let mut h = gw_hash::blake2b::new_blake2b();
    h.update(b"GW_SETTLEMENT_V3/value\0");
    h.update(bytes);
    let mut out = [0; 32];
    h.finalize(&mut out);
    out
}
fn key(domain: &[u8], deployment: Hash, body: &[u8]) -> Hash {
    let mut h = gw_hash::blake2b::new_blake2b();
    h.update(b"GW_SETTLEMENT_V3/key\0");
    h.update(&(domain.len() as u32).to_le_bytes());
    h.update(domain);
    h.update(&deployment);
    h.update(body);
    let mut out = [0; 32];
    h.finalize(&mut out);
    out
}
pub fn metadata_key(deployment: Hash, rollup: Hash) -> Hash {
    key(b"metadata", deployment, &rollup)
}
pub fn source_key(d: Hash, s: &Source) -> Hash {
    let mut v = [0; 92];
    v[..4].copy_from_slice(&s.account.to_le_bytes());
    v[4..36].copy_from_slice(&s.script_hash);
    v[36..56].copy_from_slice(&s.address);
    v[56..60].copy_from_slice(&s.asset.to_le_bytes());
    v[60..].copy_from_slice(&s.type_hash);
    key(b"source", d, &v)
}
pub fn lot_key(d: Hash, l: &Lot) -> Hash {
    key(b"lot", d, &l.id)
}
pub fn claim_key(d: Hash, c: &Claim) -> Hash {
    let mut v = [0; 92];
    v[..8].copy_from_slice(&c.epoch.to_le_bytes());
    v[8..40].copy_from_slice(&c.source);
    v[40..72].copy_from_slice(&c.lot);
    v[72..].copy_from_slice(&c.address);
    key(b"claim", d, &v)
}
pub fn epoch_key(d: Hash, id: u64) -> Hash {
    key(b"epoch", d, &id.to_le_bytes())
}
pub fn asset_key(d: Hash, id: u32) -> Hash {
    key(b"asset", d, &id.to_le_bytes())
}
pub fn manifest_hash(b: &[u8]) -> Hash {
    hash(b)
}
