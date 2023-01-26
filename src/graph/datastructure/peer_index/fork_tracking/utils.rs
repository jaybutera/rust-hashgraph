use thiserror::Error;

use self::multiples::Multiples;
use crate::graph::event;

// To not accidentally use private stuff
mod multiples {
    use std::collections::BTreeMap;

    use thiserror::Error;

    #[derive(Debug, Error, PartialEq)]
    #[error("The index is not a multiple of specified multiplier")]
    pub struct NotMultiple;

    /// Stores items with each nth index (number/height/etc.).
    ///
    /// Intended to store subsequent elements starting not from the
    /// beginning. For example, for `multiplier` 11 we may want to
    /// store elements with indices 33, 44, 55, 66, 77, 88.
    ///
    /// Essentially [BTreeMap] but restricted to numeric indexes
    /// that are multiples of the multiplier.
    #[derive(Debug, Clone, PartialEq)]
    pub struct Multiples<TItem, TIndex = u64, TMul = u32> {
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
        pub fn new(multiplier: TMul) -> Option<Self> {
            if multiplier == 0.into() {
                return None;
            }
            Some(Self {
                multiplier,
                items: BTreeMap::new(),
            })
        }

        pub fn try_insert(&mut self, index: TIndex, element: &TItem) -> Result<(), NotMultiple> {
            if index % self.multiplier.into() != 0.into() {
                return Err(NotMultiple);
            }
            self.items.insert(index, element.clone());
            Ok(())
        }

        /// Splits the collection into two at the given key. Returns everything after the given key, including the key.
        pub fn split_off(&mut self, index: TIndex) -> Self {
            let new_items = self.items.split_off(&index);
            Self {
                multiplier: self.multiplier,
                items: new_items,
            }
        }

        /// All entries, in order by their index
        pub fn entries(&self) -> std::collections::btree_map::Iter<TIndex, TItem> {
            self.items.iter()
        }
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum SplitError {
    // Also note implies that single-event extension is not splittable
    #[error(
        "Parent height is out of bounds for splitting the extension. \
        Note that parent cannot be the last event of the extension"
    )]
    HeightOutOfBounds,
    #[error(
        "Split occurs at the start of the extension, thus split \
        parent must be equal to the extension start."
    )]
    ParentStartMismatch,
    #[error(
        "Split occurs at the end of the extension, thus split \
        child must be equal to the extension end."
    )]
    ChildEndMismatch,
}

/// Node that represents a sequence of events without branching/forking
#[derive(Debug, Clone, PartialEq)]
pub struct Extension {
    first: event::Hash,
    first_height: usize,
    last: event::Hash,
    length: usize,
    mutltiples: Multiples<event::Hash, usize, u8>,
}

impl Extension {
    fn from_event_with_multiplier(
        event: event::Hash,
        height: usize,
        multiplier: u8,
    ) -> Option<Self> {
        let mut multiples = Multiples::new(multiplier)?;
        // it knows better whether to insert
        let _ = multiples.try_insert(height, &event);
        Some(Self {
            first: event.clone(),
            first_height: height,
            last: event,
            length: 1,
            mutltiples: multiples,
        })
    }

    /// Construct extension consisting of a single event
    pub fn from_event(event: event::Hash, height: usize) -> Self {
        Self::from_event_with_multiplier(event, height, 10).expect("10!=0")
    }
    /// Add event to the end of extension.
    pub fn push_event(&mut self, event: event::Hash) {
        // it knows if insert
        let _ = self
            .mutltiples
            .try_insert(self.first_height + self.length, &event);
        self.last = event;
        self.length += 1;
    }

    /// Split the extension [A, D] into two: [A, B] and [C, D], where B is `parent` and C is `child`.
    ///
    /// Returns two extensions: `(before the split, after)`
    pub fn split_at(
        mut self,
        parent: event::Hash,
        parent_height: usize,
        child: event::Hash,
    ) -> Result<(Self, Self), SplitError> {
        if parent_height < self.first_height || parent_height >= self.first_height + self.length - 1
        {
            return Err(SplitError::HeightOutOfBounds);
        }
        if parent_height == self.first_height && parent != self.first {
            return Err(SplitError::ParentStartMismatch);
        }
        // `self.first_height < parent_heigth < self.first_height + self.length`
        // is true from the first condition. These are all integers, so
        // length is at least 2. It means `length-2` won't panic
        if parent_height == self.first_height + (self.length - 2) && child != self.last {
            return Err(SplitError::ChildEndMismatch);
        }
        let first_part_length = parent_height - self.first_height + 1;
        let second_multiples = self.mutltiples.split_off(parent_height + 1);
        let first_part = Extension {
            first: self.first,
            first_height: self.first_height,
            last: parent,
            length: first_part_length,
            mutltiples: self.mutltiples,
        };
        let second_part = Extension {
            first: child,
            first_height: parent_height + 1,
            last: self.last,
            length: self
                .length
                .checked_sub(first_part_length)
                .expect("Incorrect source length or some height"),
            mutltiples: second_multiples,
        };
        Ok((first_part, second_part))
    }

    pub fn first_event(&self) -> &event::Hash {
        &self.first
    }

    pub fn first_height(&self) -> &usize {
        &self.first_height
    }

    pub fn last_event(&self) -> &event::Hash {
        &self.last
    }

    pub fn length(&self) -> &usize {
        &self.length
    }

    pub fn multiples(&self) -> std::collections::btree_map::Iter<usize, event::Hash> {
        self.mutltiples.entries()
    }
}

#[cfg(test)]
pub mod test_utils {
    use hex_literal::hex;

    use crate::graph::event;

    pub const TEST_HASH_A: event::Hash = event::Hash::from_array(
        hex![
            "1c09ecaba3131425e5f04afb9e6ea029c363cdfbb17a04aff4946847d20bd85be6dbd9529a9b5bea3d63c967645ce28891e9994844fc6e0fdd0468d60fdf0300"
        ]
    );
    pub const TEST_HASH_B: event::Hash = event::Hash::from_array(
        hex![
            "66b4d625d5729f5a36fd918fbbda2cd38f636743708d489f9a35d0a62e7ca319b9db7939fbd129d0e8a3b4e00586acc88439e2bb7f9ba2beada06f1c34a6c065"
        ]
    );
    pub const TEST_HASH_C: event::Hash = event::Hash::from_array(
        hex![
            "65a3247180d90327a35f3662920336feb5e9487630294cdeb08ca25720998633ff95108c950200453ccb2ace1a4c774f4ae4887203900506576b38dd7fe93fd3"
        ]
    );
    pub const TEST_HASH_D: event::Hash = event::Hash::from_array(
        hex![
            "f537c5c9cf69000588d0bb69b835b7f3d062540e981acb82d748eeb102ad2a3b82cc228dba870fd0da5a9e7c5bf2669d3cb852520838599ecb52230ed15be1f4"
        ]
    );
    pub const TEST_HASH_E: event::Hash = event::Hash::from_array(
        hex![
            "f660614747149b2b9d324b503891d923bf96626f7cc8e0a5c2bc90e2803105d68e2710cd986b356626d067ef15b52af4caf29085e3ee8925104c3982020eb991"
        ]
    );
    pub const TEST_HASH_F: event::Hash = event::Hash::from_array(
        hex![
            "45d854c1bb52aa932940c6d80662961301f96f46d7f7fc9b5fc0a17d12d073fdc581dab54ee1e414a562ce354c74b2994935e4a8a843040336122add8e0a7086"
        ]
    );
    // For debugging
    #[allow(unused)]
    pub const NAMES: [(event::Hash, &str); 6] = [
        (TEST_HASH_A, "A"),
        (TEST_HASH_B, "B"),
        (TEST_HASH_C, "C"),
        (TEST_HASH_D, "D"),
        (TEST_HASH_E, "E"),
        (TEST_HASH_F, "F"),
    ];
}

#[cfg(test)]
mod tests {
    use itertools::Itertools;
    use multiples::NotMultiple;

    use super::*;
    use test_utils::*;

    #[test]
    fn multiples_construct() {
        let mut m = multiples::Multiples::<&str>::new(3).unwrap();
        m.try_insert(0, &"a").unwrap();
        m.try_insert(3, &"b").unwrap();
        m.try_insert(6, &"c").unwrap();
        // should work in any order
        m.try_insert(12, &"e").unwrap();
        m.try_insert(9, &"d").unwrap();
        assert_eq!(m.try_insert(1, &"fail"), Err(NotMultiple));
        assert_eq!(
            m.entries().collect_vec(),
            vec![(&0, &"a"), (&3, &"b"), (&6, &"c"), (&9, &"d"), (&12, &"e"),]
        );
    }

    #[test]
    fn multiples_splits_off() {
        fn construct_mul() -> Multiples<&'static str> {
            let mut m = multiples::Multiples::<&str>::new(3).unwrap();
            let entries = [(0, &"a"), (3, &"b"), (6, &"c"), (9, &"d"), (12, &"e")];
            for (index, item) in entries {
                m.try_insert(index, item)
                    .expect(&format!("Failed to insert {index}, \"{item}\""));
            }
            m
        }
        let mut m = construct_mul();
        let m2 = m.split_off(9);
        assert_eq!(
            m.entries().collect_vec(),
            vec![(&0, &"a"), (&3, &"b"), (&6, &"c"),]
        );
        assert_eq!(m2.entries().collect_vec(), vec![(&9, &"d"), (&12, &"e"),]);

        let mut m = construct_mul();
        let m2 = m.split_off(7);
        assert_eq!(
            m.entries().collect_vec(),
            vec![(&0, &"a"), (&3, &"b"), (&6, &"c"),]
        );
        assert_eq!(m2.entries().collect_vec(), vec![(&9, &"d"), (&12, &"e"),]);
    }

    #[test]
    fn extension_constructs() {
        let mut ext = Extension::from_event_with_multiplier(TEST_HASH_A, 0, 3).unwrap();
        ext.push_event(TEST_HASH_B);
        ext.push_event(TEST_HASH_C);
        ext.push_event(TEST_HASH_D);
        ext.push_event(TEST_HASH_E);
        ext.push_event(TEST_HASH_F);

        assert_eq!(
            ext.multiples().collect_vec(),
            vec![(&0, &TEST_HASH_A), (&3, &TEST_HASH_D),]
        );
        assert_eq!(ext.first_event(), &TEST_HASH_A);
        assert_eq!(ext.last_event(), &TEST_HASH_F);
        assert_eq!(ext.first_height(), &0);
        assert_eq!(ext.length(), &6);
    }

    #[test]
    fn extension_splits_off() {
        fn validate_ext(
            ext: &Extension,
            multiples_entries: Vec<(&usize, &event::Hash)>,
            first: &event::Hash,
            last: &event::Hash,
            first_height: usize,
            length: usize,
        ) {
            assert_eq!(ext.multiples().collect_vec(), multiples_entries);
            assert_eq!(ext.first_event(), first);
            assert_eq!(ext.last_event(), last);
            assert_eq!(ext.first_height(), &first_height);
            assert_eq!(ext.length(), &length);
        }

        let mut ext = Extension::from_event_with_multiplier(TEST_HASH_A, 0, 3).unwrap();
        ext.push_event(TEST_HASH_B);
        ext.push_event(TEST_HASH_C);
        ext.push_event(TEST_HASH_D);
        ext.push_event(TEST_HASH_E);
        ext.push_event(TEST_HASH_F);
        // non mut
        let ext = ext;

        // split at B-C
        let (ext_before, ext_after) = ext.clone().split_at(TEST_HASH_B, 1, TEST_HASH_C).unwrap();
        validate_ext(
            &ext_before,
            vec![(&0, &TEST_HASH_A)],
            &TEST_HASH_A,
            &TEST_HASH_B,
            0,
            2,
        );
        validate_ext(
            &ext_after,
            vec![(&3, &TEST_HASH_D)],
            &TEST_HASH_C,
            &TEST_HASH_F,
            2,
            4,
        );

        // split at C-D
        let (ext_before, ext_after) = ext.clone().split_at(TEST_HASH_C, 2, TEST_HASH_D).unwrap();
        validate_ext(
            &ext_before,
            vec![(&0, &TEST_HASH_A)],
            &TEST_HASH_A,
            &TEST_HASH_C,
            0,
            3,
        );
        validate_ext(
            &ext_after,
            vec![(&3, &TEST_HASH_D)],
            &TEST_HASH_D,
            &TEST_HASH_F,
            3,
            3,
        );

        // split at D-E
        let (ext_before, ext_after) = ext.clone().split_at(TEST_HASH_D, 3, TEST_HASH_E).unwrap();
        validate_ext(
            &ext_before,
            vec![(&0, &TEST_HASH_A), (&3, &TEST_HASH_D)],
            &TEST_HASH_A,
            &TEST_HASH_D,
            0,
            4,
        );
        validate_ext(&ext_after, vec![], &TEST_HASH_E, &TEST_HASH_F, 4, 2);
    }

    #[test]
    fn extension_errors_returned() {
        let mut ext = Extension::from_event_with_multiplier(TEST_HASH_A, 0, 3).unwrap();
        ext.push_event(TEST_HASH_B);
        ext.push_event(TEST_HASH_C);
        ext.push_event(TEST_HASH_D);
        ext.push_event(TEST_HASH_E);
        ext.push_event(TEST_HASH_F);

        fn check_state(ext: &Extension) {
            assert_eq!(
                ext.multiples().collect_vec(),
                vec![(&0, &TEST_HASH_A), (&3, &TEST_HASH_D),]
            );
            assert_eq!(ext.first_event(), &TEST_HASH_A);
            assert_eq!(ext.last_event(), &TEST_HASH_F);
            assert_eq!(ext.first_height(), &0);
            assert_eq!(ext.length(), &6);
        }

        // Check state just in case
        check_state(&ext);

        assert_eq!(
            ext.clone().split_at(TEST_HASH_A, 1, TEST_HASH_C),
            Err(SplitError::ParentStartMismatch)
        );
        assert_eq!(
            ext.clone().split_at(TEST_HASH_E, 4, TEST_HASH_D),
            Err(SplitError::ChildEndMismatch)
        );
        assert_eq!(
            ext.clone().split_at(TEST_HASH_E, 4, TEST_HASH_D),
            Err(SplitError::ChildEndMismatch)
        );
    }
}
