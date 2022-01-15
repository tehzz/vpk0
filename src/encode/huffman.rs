use crate::{
    encode::{BitSize, Frequency, LzssPass},
    errors::EncodeTreeParseErr,
};
use crate::{
    errors::VpkError,
    format::{TreeEntry, VpkTree},
};
use std::{
    cmp::Ordering,
    collections::{BinaryHeap, HashMap},
    fmt,
    mem::size_of,
    str::FromStr,
};

use smallvec::{smallvec, SmallVec};

type SizeFreq = (BitSize, Frequency);
// size in bits => (bit size for encoded value, huffcode prefix prior encoded value bitsize)
pub(super) type CodeMap = HashMap<BitSize, (BitSize, HuffCode)>;

#[derive(Debug)]
pub(super) struct EncodedMaps {
    // moveback
    pub offsets: MapTree,
    // size
    pub lengths: MapTree,
}

impl EncodedMaps {
    ///
    pub(super) fn new(
        offsets: Option<&str>,
        lengths: Option<&str>,
        p1: &LzssPass,
    ) -> Result<Self, VpkError> {
        let offsets = offsets
            .map(str::parse::<MapTree>)
            .map(|t| t.map(|t| t.fill_missing(&p1.moveback_bitfreq)))
            .transpose()?
            .unwrap_or_else(|| Tree::from_found_codes(&p1.moveback_bitfreq).into());
        let lengths = lengths
            .map(str::parse::<MapTree>)
            .map(|t| t.map(|t| t.fill_missing(&p1.size_bitfreq)))
            .transpose()?
            .unwrap_or_else(|| Tree::from_found_codes(&p1.size_bitfreq).into());

        Ok(Self { offsets, lengths })
    }
}

#[derive(Debug)]
pub(super) struct MapTree {
    map: CodeMap,
    pub tree: VpkTree,
}

impl MapTree {
    pub fn get(&self, bitsize: BitSize) -> Option<(BitSize, HuffCode)> {
        self.map.get(&bitsize).copied()
    }

    fn fill_missing(mut self, found: &HashMap<BitSize, Frequency>) -> Self {
        // TODO: errors?
        let max = self
            .map
            .keys()
            .copied()
            .max()
            .expect("at least one bit size in MapTree");

        for &bitsize in found.keys() {
            if bitsize > max {
                panic!("tried to insert {} into tree with max of {}", bitsize, max)
            }

            let mut check = bitsize;
            while check <= max {
                if let Some(&value) = self.map.get(&check) {
                    self.map.insert(bitsize, value);
                    break;
                }

                check += 1;
            }
        }

        self
    }

    /// Create an empty Tree (i.e., no found matches in a buffer)
    fn empty() -> Self {
        Self {
            map: CodeMap::new(),
            tree: VpkTree::empty(),
        }
    }
}

impl fmt::Display for MapTree {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.map.is_empty() {
            return writeln!(f, "empty tree");
        }

        for (key, (size, code)) in &self.map {
            writeln!(f, "{} : {} (read next {} bytes)", key, code, size)?
        }
        Ok(())
    }
}

impl From<Option<Tree>> for MapTree {
    fn from(opt: Option<Tree>) -> Self {
        opt.map(|tree| {
            let map = tree.generate_code_map();
            let tree = tree.into();
            Self { map, tree }
        }).unwrap_or_else(Self::empty)
    }
}

impl FromStr for MapTree {
    type Err = EncodeTreeParseErr;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let t = parse_treestr(s)?;
        let map = t.generate_code_map();
        let tree = t.into();

        Ok(Self { map, tree })
    }
}

struct Tree {
    root: TreeNode,
}

impl Tree {
    fn from_heap(mut heap: BinaryHeap<TreeNode>) -> Option<Self> {
        if heap.is_empty() {
            return None;
        }

        while heap.len() >= 2 {
            let l = heap.pop().unwrap();
            let r = heap.pop().unwrap();

            let new = TreeNode::combine(l, r);
            heap.push(new);
        }

        let root = heap.pop().unwrap();

        Some(Self { root })
    }

    fn generate_code_map(&self) -> CodeMap {
        let mut map = HashMap::new();
        self.root.generate_code(HuffCode::new(), &mut map);
        map
    }

    fn from_found_codes(map: &HashMap<BitSize, Frequency>) -> Option<Self> {
        let copied_tupple = |(&a, &b)| (a, b);

        let heap = map.iter().map(copied_tupple).map(TreeNode::from).collect();

        Self::from_heap(heap)
    }

    /*
    fn canonical_codes(&self) -> HashMap<BitSize, HuffCode> {
        let codes = self.generate_code_map();
        let mut buf: Vec<_> = codes.into_iter().map(|(s, c)| (s, c.len())).collect();
        buf.sort_unstable_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));

        buf.into_iter()
            .scan((0, None), |(code, prev_len), (size, cur_len)| {
                *code = prev_len
                    .map(|prev| (*code + 1) << (cur_len - prev))
                    .unwrap_or(*code);
                *prev_len = Some(cur_len);

                Some((size, HuffCode::create(*code, cur_len)))
            })
            .collect()
    }
    */
}

/*
impl FromIterator<SizeFreq> for Option<Tree> {
    fn from_iter<I: IntoIterator<Item = SizeFreq>>(iter: I) -> Self {
        let heap = iter.into_iter().map(TreeNode::from).collect();
        Self::from_heap(heap)
    }
}
*/

#[allow(clippy::from_over_into)]
impl Into<VpkTree> for Tree {
    fn into(self) -> VpkTree {
        let mut flat = Vec::new();
        self.root.flatten(&mut flat);
        flat.into()
    }
}
enum TreeNode {
    Leaf {
        size: BitSize,
        freq: Frequency,
    },
    CombinedLeaf {
        size: BitSize,
        freq: Frequency,
        lesser: SmallVec<[BitSize; 8]>,
    },
    Node {
        freq: Frequency,
        left: Box<TreeNode>,
        right: Box<TreeNode>,
    },
}

impl TreeNode {
    fn freq(&self) -> Frequency {
        match self {
            Self::Leaf { freq, .. } => *freq,
            Self::CombinedLeaf { freq, .. } => *freq,
            Self::Node { freq, .. } => *freq,
        }
    }

    fn size(&self) -> Option<BitSize> {
        match self {
            Self::Leaf { size, .. } => Some(*size),
            Self::CombinedLeaf { size, .. } => Some(*size),
            Self::Node { .. } => None,
        }
    }

    fn lessers(&self) -> Option<&[BitSize]> {
        match self {
            Self::CombinedLeaf { lesser, .. } => Some(lesser),
            _ => None,
        }
    }

    fn combine(l: Self, r: Self) -> Self {
        let make_node = |l: Self, r: Self| Self::Node {
            freq: l.freq() + r.freq(),
            left: Box::new(l),
            right: Box::new(r),
        };

        pair_lesser_sizes(&l, &r).unwrap_or_else(|| make_node(l, r))
    }

    fn generate_code(&self, prefix: HuffCode, map: &mut CodeMap) {
        match self {
            Self::Leaf { size, .. } => {
                map.insert(*size, (*size, prefix));
            }
            Self::CombinedLeaf { size, lesser, .. } => {
                map.insert(*size, (*size, prefix));
                map.extend(lesser.into_iter().map(|s| (*s, (*size, prefix))));
            }
            Self::Node { left, right, .. } => {
                left.generate_code(prefix.extend(false), map);
                right.generate_code(prefix.extend(true), map);
            }
        }
    }

    fn flatten(&self, arr: &mut Vec<TreeEntry>) -> usize {
        match self {
            Self::Leaf { size, .. } | Self::CombinedLeaf { size, .. } => {
                arr.push(TreeEntry::Leaf(*size));
                arr.len() - 1
            }
            Self::Node { left, right, .. } => {
                let left = left.flatten(arr);
                let right = right.flatten(arr);
                arr.push(TreeEntry::Node { left, right });
                arr.len() - 1
            }
        }
    }
}

impl From<SizeFreq> for TreeNode {
    fn from(sf: SizeFreq) -> Self {
        Self::Leaf {
            size: sf.0,
            freq: sf.1,
        }
    }
}

impl Ord for TreeNode {
    fn cmp(&self, other: &Self) -> Ordering {
        self.freq().cmp(&other.freq()).reverse()
    }
}

impl PartialOrd for TreeNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for TreeNode {
    fn eq(&self, other: &Self) -> bool {
        self.freq() == other.freq()
    }
}

impl Eq for TreeNode {}

// try save bits if two `TreeNode::Leaf`s are next to each other in the tree
fn pair_lesser_sizes(l: &TreeNode, r: &TreeNode) -> Option<TreeNode> {
    order_leaves(l, r).and_then(|(higher, lower)| {
        let (hs, ls) = (higher.size().unwrap(), lower.size().unwrap());
        let (hf, lf) = (higher.freq(), lower.freq());
        let bit_diff = (hs - ls) as i64;
        let bits_gained = hf as i64; // one bit is saved for each occurrence
        let bits_lost = (bit_diff - 1) * lf as i64;

        if bits_gained - bits_lost >= 0 {
            let hl = higher.lessers().into_iter().flatten().copied();
            let ll = lower.lessers().into_iter().flatten().copied();
            let mut lesser = smallvec![ls];
            lesser.extend(hl.chain(ll));
            Some(TreeNode::CombinedLeaf {
                size: hs,
                freq: hf + lf,
                lesser,
            })
        } else {
            None
        }
    })
}

// return (higher, lower) based on their bitsizes
fn order_leaves<'a>(l: &'a TreeNode, r: &'a TreeNode) -> Option<(&'a TreeNode, &'a TreeNode)> {
    l.size()
        .and_then(|ls| r.size().map(|rs| if ls >= rs { (l, r) } else { (r, l) }))
}

type BitCodeBacking = u32;
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub(super) struct HuffCode {
    pub code: BitCodeBacking,
    size: u8,
}

impl HuffCode {
    const MAX_SIZE: usize = size_of::<BitCodeBacking>() * 8;

    #[inline(always)]
    fn len(&self) -> usize {
        self.size as usize
    }

    #[inline(always)]
    pub(super) fn bitlen(&self) -> u32 {
        self.size as u32
    }

    fn push(&mut self, bit: bool) {
        self.size += 1;
        if self.len() >= Self::MAX_SIZE {
            panic!("exceded bit size for huffman code");
        }
        self.code <<= 1;
        self.code |= bit as u32;
    }

    fn extend(mut self, bit: bool) -> Self {
        self.push(bit);
        self
    }

    /*
    fn create(code: u32, len: usize) -> Self {
        if len >= Self::MAX_SIZE {
            panic!("exceded bit size for huffman code");
        }

        Self {
            code,
            size: len as u8,
        }
    }
    */

    fn new() -> Self {
        Self { code: 0, size: 0 }
    }
}

impl fmt::Display for HuffCode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:0width$b}", self.code, width = self.len())
    }
}

// for parsing "tree strings" into `Tree` pointer structure
// e.g. ((7, ((4, 1), 5)), ((10, 6), 9))
fn parse_treestr(s: &str) -> Result<Tree, EncodeTreeParseErr> {
    let lexed = lex_treestr(s)?;
    let mut lex_iter = lexed.iter().copied();

    parse_tree(&mut lex_iter)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LexToken(usize, Token);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token {
    OpenParen,
    CloseParen,
    Comma,
    Whitespace,
    Number(BitSize),
}

impl Token {
    fn as_str(&self) -> &'static str {
        match self {
            Self::OpenParen => "(",
            Self::CloseParen => ")",
            Self::Comma => ",",
            Self::Whitespace => "whitespace",
            Self::Number(..) => "number",
        }
    }
}

type LexIter<'a> = dyn Iterator<Item = LexToken> + 'a;

fn lex_treestr(s: &str) -> Result<Vec<LexToken>, EncodeTreeParseErr> {
    use EncodeTreeParseErr as E;
    use Token::*;

    let get_pos = |csr: &str| s.len() - csr.len();
    let count_ws = |s: &str| s.chars().take_while(|c| c.is_whitespace()).count();
    let parse_num = |s: &str| {
        let n = s.chars().take_while(|c| c.is_digit(10)).count();
        s[..n].parse().map(Number).map(|t| (n, t))
    };

    let mut csr = s;
    let mut output = Vec::with_capacity(s.len());
    while let Some(c) = csr.chars().next() {
        let (offset, token) = match c {
            '(' => Ok((1, OpenParen)),
            ')' => Ok((1, CloseParen)),
            ',' => Ok((1, Comma)),
            _ if c.is_whitespace() => Ok((count_ws(csr), Whitespace)),
            _ if c.is_digit(10) => parse_num(csr).map_err(|e| E::LexNum(e, get_pos(csr))),
            _ => Err(E::LexUnexp(c, get_pos(csr))),
        }?;

        if token != Whitespace {
            let position = get_pos(csr);
            output.push(LexToken(position, token));
        }
        csr = &csr[offset..];
    }

    Ok(output)
}

// node -> (node, node) | leaf
// leaf -> NUMBER
fn parse_tree(iter: &mut LexIter) -> Result<Tree, EncodeTreeParseErr> {
    parse_node(iter).map(|root| Tree { root })
}

fn parse_node(iter: &mut LexIter) -> Result<TreeNode, EncodeTreeParseErr> {
    use EncodeTreeParseErr as E;
    use Token::*;

    let early_end = || Err(E::ParseUnexpEnd);
    let unexp = |t: LexToken| Err(E::ParseUnexp(t.1.as_str(), t.0));

    match iter.next() {
        Some(LexToken(_, Number(size))) => Ok(TreeNode::Leaf { size, freq: 0 }),
        Some(LexToken(_, OpenParen)) => {
            let left = parse_node(iter)?;

            // check for comma
            match iter.next() {
                Some(LexToken(_, Comma)) => (),
                Some(t) => {
                    unexp(t)?;
                }
                None => {
                    early_end()?;
                }
            };

            let right = parse_node(iter)?;

            // check for end of node
            match iter.next() {
                Some(LexToken(_, CloseParen)) => (),
                Some(t) => {
                    unexp(t)?;
                }
                None => {
                    early_end()?;
                }
            };

            Ok(TreeNode::Node {
                left: Box::new(left),
                right: Box::new(right),
                freq: 0,
            })
        }
        Some(t) => unexp(t),
        None => early_end(),
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parse_leaf_only_treestr() {
        let inputs = ["5", "7"];
        let outputs = [vec![(5, 0b0)], vec![(7, 0b0)]];

        for (s, parsed) in inputs.iter().zip(&outputs) {
            check_tree_parsing(s, parsed);
        }
    }

    #[test]
    fn parse_user_treestr() {
        let inputs = [
            "(1, (4, (6, (3, 7))))",
            "(3, (2, 5))",
            "((7, ((4, 1), 5)), ((10, 6), 9))",
            "((9, 11), (13, (14, 16)))",
        ];
        let outputs = [
            vec![(1, 0b0), (4, 0b10), (6, 0b110), (3, 0b1110), (7, 0b1111)],
            vec![(3, 0b0), (2, 0b10), (5, 0b11)],
            vec![
                (7, 0b00),
                (4, 0b0100),
                (1, 0b0101),
                (5, 0b011),
                (10, 0b100),
                (6, 0b101),
                (9, 0b11),
            ],
            vec![(9, 0b00), (11, 0b01), (13, 0b10), (14, 0b110), (16, 0b111)],
        ];

        for (s, parsed) in inputs.iter().zip(&outputs) {
            check_tree_parsing(s, parsed);
        }
    }

    #[test]
    fn filling_user_tree() -> Result<(), VpkError> {
        // Nintendo's typical trees do not have a single entry for each possible bitsize in the file
        // so, these bitsizes need to be filled into the code based on existing huffman codes
        let inputs = &["(3, 5)", "(1, (4, 7))"];
        let found_sizes: &[HashMap<BitSize, Frequency>] = &[
            [(2, 5), (3, 8), (4, 4), (5, 1)].iter().copied().collect(),
            [(1, 8), (3, 1), (4, 4), (6, 3), (7, 2)]
                .iter()
                .copied()
                .collect(),
        ];

        for (s, found) in inputs.iter().zip(found_sizes) {
            let tree = s.parse::<MapTree>()?.fill_missing(found);
            println!("Tree for {}\n\t{:?}", s, tree);
        }

        Ok(())
    }

    fn check_tree_parsing(s: &str, parsed: &[(BitSize, u32)]) {
        let tree = match parse_treestr(s) {
            Ok(t) => t,
            Err(e) => {
                let mut cause = Some(&e as &dyn std::error::Error);
                while let Some(e) = cause {
                    eprintln!("{}", e);
                    cause = e.source();
                }
                panic!(
                    "issue parsing {}\nrun with '-- --nocapture' to see details",
                    s
                );
            }
        };
        let map = tree.generate_code_map();
        for (key, expected) in parsed {
            let found = map.get(key);
            assert!(
                found.is_some(),
                "didn't create huffcode for {} (from '{}')",
                key,
                s
            );
            let (_, found) = found.unwrap();
            assert_eq!(
                *expected, found.code,
                "incorrect parsed huffcode for {}: got {:0b} expected {:0b}\n{}",
                key, found.code, expected, s
            );
        }
    }
}
