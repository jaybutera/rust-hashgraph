
#[derive(Debug, Error, PartialEq)]
pub enum MultiplesInsertError {
    #[error("The index is not a multiple of specified multiplier")]
    NotMultiple,
}

/// Stores items with each nth index (number/height/etc.).
///
/// Intended to store subsequent elements starting not from the
/// beginning. For example, for `multiplier` 11 we may want to
/// store elements with indices 33, 44, 55, 66, 77, 88.
#[derive(Debug, Clone, PartialEq)]
struct Multiples<TItem, TIndex = u64, TMul = u32>
{
    multiplier: TMul,
    items: BTreeMap<TIndex, TItem>,
}

impl<TItem, TIndex, TMul> Multiples<TItem, TIndex, TMul> 
where
    TMul: Into<TIndex> + PartialEq + From<u8> + Copy,
    TIndex: std::ops::Rem + Ord + Copy,
    <TIndex as std::ops::Rem>::Output: PartialEq + From<u8>,
    TItem: Clone,
    {
    // `None` if `multiplier` is 0
    fn new(multiplier: TMul) -> Option<Self> {
        if multiplier == 0.into() {
            return None;
        }
        Some(Self {
            multiplier,
            items: BTreeMap::new(),
        })
    }

    fn try_insert(&mut self, index: TIndex, element: &TItem) -> Result<(), MultiplesInsertError> {
        if index % self.multiplier.into() != 0.into() {
            return Err(MultiplesInsertError::NotMultiple)
        }
        self.items.insert(index, element.clone());
        Ok(())
    }

    /// Splits the collection into two at the given key. Returns everything after the given key, including the key.
    fn split_off(&mut self, index: TIndex) -> Self {
        let new_items = self.items.split_off(&index);
        Self { multiplier: self.multiplier, items: new_items }
    }
}
