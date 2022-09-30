/// https://github.com/Flouse/bn/tree/0.6.0
use substrate_bn::{
    arith::U256, pairing_batch, AffineG1, AffineG2, Fq, Fq2, Fr, Group, Gt, G1, G2,
};

pub struct Error(pub &'static str);

fn read_pt(buf: &[u8]) -> Result<G1, Error> {
    let px = Fq::from_slice(&buf[0..32]).map_err(|_| Error("invalid pt"))?;
    let py = Fq::from_slice(&buf[32..64]).map_err(|_| Error("invalid pt"))?;
    Ok(if px == Fq::zero() && py == Fq::zero() {
        G1::zero()
    } else {
        AffineG1::new(px, py)
            .map_err(|_| Error("invalid pt"))?
            .into()
    })
}

fn read_fr(buf: &[u8]) -> Result<Fr, Error> {
    Fr::from_slice(buf).map_err(|_| Error("invalid fr"))
}

pub fn add(input: &[u8]) -> Result<[u8; 64], Error> {
    let mut buffer = [0u8; 128];
    if input.len() < 128 {
        buffer[0..input.len()].copy_from_slice(input);
    } else {
        buffer[0..128].copy_from_slice(&input[0..128]);
    }
    let p1 = read_pt(&buffer[0..64])?;
    let p2 = read_pt(&buffer[64..128])?;

    let mut buffer = [0u8; 64];
    if let Some(sum) = AffineG1::from_jacobian(p1 + p2) {
        sum.x().to_big_endian(&mut buffer[0..32]).unwrap();
        sum.y().to_big_endian(&mut buffer[32..64]).unwrap();
    }
    Ok(buffer)
}

pub fn mul(input: &[u8]) -> Result<[u8; 64], Error> {
    let mut buffer = [0u8; 96];
    if input.len() < 96 {
        buffer[0..input.len()].copy_from_slice(input);
    } else {
        buffer[0..96].copy_from_slice(&input[0..96]);
    }
    let pt = read_pt(&buffer[0..64])?;
    let fr = read_fr(&buffer[64..96])?;

    let mut buffer = [0u8; 64];
    if let Some(sum) = AffineG1::from_jacobian(pt * fr) {
        sum.x().to_big_endian(&mut buffer[0..32]).unwrap();
        sum.y().to_big_endian(&mut buffer[32..64]).unwrap();
    }
    Ok(buffer)
}

pub fn pairing(input: &[u8]) -> Result<[u8; 32], Error> {
    if input.len() % 192 != 0 {
        return Err(Error(
            "Invalid input length, must be multiple of 192 (3 * (32*2))",
        ));
    }
    let elements = input.len() / 192; // (a, b_a, b_b - each 64-byte affine coordinates)

    let ret = if input.is_empty() {
        U256::one()
    } else {
        let mut vals = Vec::with_capacity(elements);
        for idx in 0..elements {
            let a_x = Fq::from_slice(&input[idx * 192..idx * 192 + 32])
                .map_err(|_| Error("Invalid a argument x coordinate"))?;

            let a_y = Fq::from_slice(&input[idx * 192 + 32..idx * 192 + 64])
                .map_err(|_| Error("Invalid a argument y coordinate"))?;

            let b_a_y = Fq::from_slice(&input[idx * 192 + 64..idx * 192 + 96])
                .map_err(|_| Error("Invalid b argument imaginary coeff x coordinate"))?;

            let b_a_x = Fq::from_slice(&input[idx * 192 + 96..idx * 192 + 128])
                .map_err(|_| Error("Invalid b argument imaginary coeff y coordinate"))?;

            let b_b_y = Fq::from_slice(&input[idx * 192 + 128..idx * 192 + 160])
                .map_err(|_| Error("Invalid b argument real coeff x coordinate"))?;

            let b_b_x = Fq::from_slice(&input[idx * 192 + 160..idx * 192 + 192])
                .map_err(|_| Error("Invalid b argument real coeff y coordinate"))?;

            let b_a = Fq2::new(b_a_x, b_a_y);
            let b_b = Fq2::new(b_b_x, b_b_y);
            let b = if b_a.is_zero() && b_b.is_zero() {
                G2::zero()
            } else {
                G2::from(
                    AffineG2::new(b_a, b_b)
                        .map_err(|_| Error("Invalid b argument - not on curve"))?,
                )
            };
            let a = if a_x.is_zero() && a_y.is_zero() {
                G1::zero()
            } else {
                G1::from(
                    AffineG1::new(a_x, a_y)
                        .map_err(|_| Error("Invalid a argument - not on curve"))?,
                )
            };
            vals.push((a, b));
        }

        let mul = pairing_batch(vals.as_ref());

        if mul == Gt::one() {
            U256::one()
        } else {
            U256::zero()
        }
    };

    let mut output = [0u8; 32];
    ret.to_big_endian(&mut output)
        .expect("Cannot fail since 0..32 is 32-byte length");
    Ok(output)
}
