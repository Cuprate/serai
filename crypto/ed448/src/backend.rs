#[doc(hidden)]
#[macro_export]
macro_rules! field {
  ($FieldName: ident, $MODULUS: ident, $WIDE_MODULUS: ident, $NUM_BITS: literal) => {
    use core::ops::{Add, AddAssign, Neg, Sub, SubAssign, Mul, MulAssign};

    use rand_core::RngCore;

    use subtle::{Choice, CtOption, ConstantTimeEq, ConditionallySelectable};

    use generic_array::{typenum::U57, GenericArray};
    use crypto_bigint::Encoding;

    use ff::{Field, PrimeField, FieldBits, PrimeFieldBits};

    // Needed to publish for some reason? Yet not actually needed
    #[allow(unused_imports)]
    use dalek_ff_group::{from_wrapper, math_op};
    use dalek_ff_group::{constant_time, from_uint, math};

    fn reduce(x: U1024) -> U512 {
      U512::from_le_slice(&x.reduce(&$WIDE_MODULUS).unwrap().to_le_bytes()[.. 64])
    }

    constant_time!($FieldName, U512);
    math!(
      $FieldName,
      $FieldName,
      |x, y| U512::add_mod(&x, &y, &$MODULUS.0),
      |x, y| U512::sub_mod(&x, &y, &$MODULUS.0),
      |x, y| {
        let wide = U512::mul_wide(&x, &y);
        reduce(U1024::from((wide.1, wide.0)))
      }
    );
    from_uint!($FieldName, U512);

    impl Neg for $FieldName {
      type Output = $FieldName;
      fn neg(self) -> $FieldName {
        *$MODULUS - self
      }
    }

    impl<'a> Neg for &'a $FieldName {
      type Output = $FieldName;
      fn neg(self) -> Self::Output {
        (*self).neg()
      }
    }

    lazy_static! {
      pub(crate) static ref ZERO: $FieldName = $FieldName(U512::ZERO);
      pub(crate) static ref ONE: $FieldName = $FieldName(U512::ONE);
      pub(crate) static ref TWO: $FieldName = $FieldName(U512::ONE.saturating_add(&U512::ONE));
    }

    impl $FieldName {
      pub fn pow(&self, other: $FieldName) -> $FieldName {
        let mut table = [*ONE; 16];
        table[1] = *self;
        for i in 2 .. 16 {
          table[i] = table[i - 1] * self;
        }

        let mut res = *ONE;
        let mut bits = 0;
        for (i, bit) in other.to_le_bits().iter().rev().enumerate() {
          bits <<= 1;
          let bit = *bit as u8;
          assert_eq!(bit | 1, 1);
          bits |= bit;

          if ((i + 1) % 4) == 0 {
            if i != 3 {
              for _ in 0 .. 4 {
                res *= res;
              }
            }
            res *= table[usize::from(bits)];
            bits = 0;
          }
        }
        res
      }
    }

    impl Field for $FieldName {
      fn random(mut rng: impl RngCore) -> Self {
        let mut bytes = [0; 128];
        rng.fill_bytes(&mut bytes);
        $FieldName(reduce(U1024::from_le_slice(bytes.as_ref())))
      }

      fn zero() -> Self {
        *ZERO
      }
      fn one() -> Self {
        *ONE
      }
      fn square(&self) -> Self {
        *self * self
      }
      fn double(&self) -> Self {
        *self + self
      }

      fn invert(&self) -> CtOption<Self> {
        CtOption::new(self.pow(-*TWO), !self.is_zero())
      }

      fn sqrt(&self) -> CtOption<Self> {
        unimplemented!()
      }

      fn is_zero(&self) -> Choice {
        self.ct_eq(&ZERO)
      }
      fn cube(&self) -> Self {
        *self * self * self
      }
      fn pow_vartime<S: AsRef<[u64]>>(&self, _exp: S) -> Self {
        unimplemented!()
      }
    }

    impl PrimeField for $FieldName {
      type Repr = GenericArray<u8, U57>;
      const NUM_BITS: u32 = $NUM_BITS;
      const CAPACITY: u32 = $NUM_BITS - 1;
      fn from_repr(bytes: Self::Repr) -> CtOption<Self> {
        let res = $FieldName(U512::from_le_slice(&[bytes.as_ref(), [0; 7].as_ref()].concat()));
        CtOption::new(res, res.0.add_mod(&U512::ZERO, &$MODULUS.0).ct_eq(&res.0))
      }
      fn to_repr(&self) -> Self::Repr {
        let mut repr = GenericArray::<u8, U57>::default();
        repr.copy_from_slice(&self.0.to_le_bytes()[.. 57]);
        repr
      }

      // True for both the Ed448 Scalar field and FieldElement field
      const S: u32 = 1;
      fn is_odd(&self) -> Choice {
        (self.to_repr()[0] & 1).into()
      }
      fn multiplicative_generator() -> Self {
        unimplemented!()
      }
      fn root_of_unity() -> Self {
        unimplemented!()
      }
    }

    impl PrimeFieldBits for $FieldName {
      type ReprBits = [u8; 56];

      fn to_le_bits(&self) -> FieldBits<Self::ReprBits> {
        let mut repr = [0; 56];
        repr.copy_from_slice(&self.to_repr()[.. 56]);
        repr.into()
      }

      fn char_le_bits() -> FieldBits<Self::ReprBits> {
        MODULUS.to_le_bits()
      }
    }
  };
}