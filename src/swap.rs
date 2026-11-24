//! Session-local swap backing store. The paging layer must pin the page and
//! serialize access through this store while its ownership is transferred.

use crate::block::{BlockDevice, BlockError, BlockRegion, SECTOR_SIZE};
use chacha20poly1305::{AeadInPlace, KeyInit, XChaCha20Poly1305};
use zeroize::{Zeroize, Zeroizing};

pub const PAGE_SIZE: usize = 4096;
const SECTORS_PER_PAGE: u64 = (PAGE_SIZE / SECTOR_SIZE) as u64;
pub type Page = [u8; PAGE_SIZE];
type Tag = chacha20poly1305::Tag;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PageIdentity {
    session: [u8; 16],
    sequence: u64,
    slot: u64,
    owner: u64,
}

impl PageIdentity {
    fn nonce(self) -> chacha20poly1305::XNonce {
        let mut nonce = [0; 24];
        nonce[..16].copy_from_slice(&self.session);
        nonce[16..].copy_from_slice(&self.sequence.to_le_bytes());
        nonce.into()
    }

    fn associated_data(self) -> [u8; 16] {
        let mut data = [0; 16];
        data[..8].copy_from_slice(&self.slot.to_le_bytes());
        data[8..].copy_from_slice(&self.owner.to_le_bytes());
        data
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct SwapPage(PageIdentity);

#[derive(Clone, Copy, Default)]
pub struct SwapSlot(Option<(PageIdentity, Tag)>);

#[derive(Debug, PartialEq, Eq)]
pub enum SwapError {
    Block(BlockError),
    Full,
    InvalidPage,
    Authentication,
    SequenceExhausted,
}

impl From<BlockError> for SwapError {
    fn from(error: BlockError) -> Self {
        Self::Block(error)
    }
}

pub struct SwapStore<'a, D> {
    region: BlockRegion<D>,
    cipher: XChaCha20Poly1305,
    slots: &'a mut [SwapSlot],
    session: [u8; 16],
    sequence: u64,
}

impl<'a, D: BlockDevice> SwapStore<'a, D> {
    /// The caller supplies a fresh key from a trusted entropy source and a
    /// unique session identifier. The store owns and clears its key on drop.
    pub fn new(
        region: BlockRegion<D>,
        key: Zeroizing<[u8; 32]>,
        session: [u8; 16],
        slots: &'a mut [SwapSlot],
    ) -> Result<Self, SwapError> {
        if slots.is_empty() || slots.len() as u64 > region.sector_count() / SECTORS_PER_PAGE {
            return Err(BlockError::OutOfRange.into());
        }
        slots.fill(SwapSlot::default());
        Ok(Self {
            region,
            cipher: XChaCha20Poly1305::new((&*key).into()),
            slots,
            session,
            sequence: 0,
        })
    }

    /// Publishes the handle after a successful flush. The caller retains its
    /// resident frame on every error and on success until VM commits eviction.
    pub fn page_out(&mut self, owner: u64, page: &Page) -> Result<SwapPage, SwapError> {
        let slot = self
            .slots
            .iter()
            .position(|slot| slot.0.is_none())
            .ok_or(SwapError::Full)?;
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or(SwapError::SequenceExhausted)?;
        let identity = PageIdentity {
            session: self.session,
            sequence: self.sequence,
            slot: slot as u64,
            owner,
        };
        let mut encrypted = Zeroizing::new(*page);
        let tag = self
            .cipher
            .encrypt_in_place_detached(
                &identity.nonce(),
                &identity.associated_data(),
                &mut *encrypted,
            )
            .map_err(|_| SwapError::Authentication)?;
        let start = identity.slot * SECTORS_PER_PAGE;
        for (offset, sector) in encrypted.as_chunks::<SECTOR_SIZE>().0.iter().enumerate() {
            self.region.write_sector(start + offset as u64, sector)?;
        }
        self.region.flush()?;
        self.slots[slot].0 = Some((identity, tag));
        Ok(SwapPage(identity))
    }

    /// Read without consuming the slot. Release it only once VM commits the
    /// mapping. The destination is cleared on all errors.
    pub fn page_in(
        &mut self,
        owner: u64,
        handle: &SwapPage,
        page: &mut Page,
    ) -> Result<(), SwapError> {
        page.zeroize();
        let (_, tag) = self.lookup(owner, handle)?;
        let start = handle.0.slot * SECTORS_PER_PAGE;
        let mut encrypted = Zeroizing::new([0; PAGE_SIZE]);
        for (offset, sector) in encrypted
            .as_chunks_mut::<SECTOR_SIZE>()
            .0
            .iter_mut()
            .enumerate()
        {
            self.region.read_sector(start + offset as u64, sector)?;
        }
        self.cipher
            .decrypt_in_place_detached(
                &handle.0.nonce(),
                &handle.0.associated_data(),
                &mut *encrypted,
                &tag,
            )
            .map_err(|_| SwapError::Authentication)?;
        page.copy_from_slice(&*encrypted);
        Ok(())
    }

    pub fn release(&mut self, owner: u64, handle: &SwapPage) -> Result<(), SwapError> {
        let (index, _) = self.lookup(owner, handle)?;
        self.slots[index].0 = None;
        Ok(())
    }

    fn lookup(&self, owner: u64, handle: &SwapPage) -> Result<(usize, Tag), SwapError> {
        let index = usize::try_from(handle.0.slot).map_err(|_| SwapError::InvalidPage)?;
        match self.slots.get(index).and_then(|slot| slot.0) {
            Some((identity, tag)) if identity == handle.0 && identity.owner == owner => {
                Ok((index, tag))
            }
            _ => Err(SwapError::InvalidPage),
        }
    }
}
