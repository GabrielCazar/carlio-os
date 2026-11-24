#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

#[cfg(target_os = "none")]
extern crate alloc;

#[cfg(target_os = "none")]
struct KernelGlobalAllocator;

#[cfg(target_os = "none")]
#[global_allocator]
static GLOBAL_ALLOCATOR: KernelGlobalAllocator = KernelGlobalAllocator;

#[cfg(target_os = "none")]
unsafe impl core::alloc::GlobalAlloc for KernelGlobalAllocator {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        kernel_heap_allocate(layout).map_or(core::ptr::null_mut(), core::ptr::NonNull::as_ptr)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: core::alloc::Layout) {
        if let Some(ptr) = core::ptr::NonNull::new(ptr) {
            let _ = unsafe { KERNEL_HEAP.lock().deallocate(ptr) };
        }
    }
}

#[cfg(target_os = "none")]
use carlio_os::errors::{ErrorLog, KernelError};
#[cfg(target_os = "none")]
use carlio_os::heap::KernelHeapSet;
#[cfg(target_os = "none")]
use carlio_os::memory::{
    FrameError, OwnedAddressSpace, PageTableAccess, PageTableManager, PhysicalFrameAllocator,
    PAGE_FLAG_NO_EXECUTE, PAGE_FLAG_USER, PAGE_FLAG_WRITABLE, PHYSICAL_FRAME_BITMAP_WORDS,
};
#[cfg(target_os = "none")]
use carlio_os::pci::PciFunction;
#[cfg(target_os = "none")]
use carlio_os::process::{
    CpuContext, KernelStackPool, OwnedProcessResource, ReapBarrier, Scheduler,
};
#[cfg(target_os = "none")]
use carlio_os::sync::IrqSpinLock;
#[cfg(target_os = "none")]
use carlio_os::vfs::{AccessContext, Credentials, Vfs};
#[cfg(target_os = "none")]
use carlio_os::{parse_command, Command};
#[cfg(target_os = "none")]
use core::arch::asm;
#[cfg(target_os = "none")]
use core::arch::global_asm;
#[cfg(target_os = "none")]
use core::fmt::{self, Write as FmtWrite};
#[cfg(target_os = "none")]
use core::panic::PanicInfo;
#[cfg(target_os = "none")]
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

#[cfg(target_os = "none")]
const COM1: u16 = 0x3f8;
#[cfg(target_os = "none")]
const INPUT_LIMIT: usize = 128;
#[cfg(target_os = "none")]
// Leituras concorrentes em APs podem disputar o único byte injetado pelo
// harness; um timeout curto mantém serial best-effort sem prender o scheduler.
const SERIAL_POLL_LIMIT: usize = 100_000;
#[cfg(target_os = "none")]
const SERIAL_READ_POLL_LIMIT: usize = 250_000;
#[cfg(target_os = "none")]
static SERIAL_RX_OWNER: carlio_os::serial_rx::SerialRxGate =
    carlio_os::serial_rx::SerialRxGate::new();
#[cfg(target_os = "none")]
const ERROR_LOG_CAPACITY: usize = 16;
#[cfg(target_os = "none")]
const MAX_BOOT_CPUS: usize = 64;
#[cfg(target_os = "none")]
const AP_STACK_SIZE: usize = 16 * 1024;
#[cfg(target_os = "none")]
const PROCESS_KERNEL_STACK_SIZE: usize = 128 * 1024;

#[cfg(target_os = "none")]
static ERROR_LOG: IrqSpinLock<ErrorLog<ERROR_LOG_CAPACITY>> = IrqSpinLock::new(ErrorLog::new());
#[cfg(target_os = "none")]
static PENDING_ERRORS: IrqSpinLock<ErrorLog<ERROR_LOG_CAPACITY>> =
    IrqSpinLock::new(ErrorLog::new());
#[cfg(target_os = "none")]
static PERSISTENT_ERRORS: IrqSpinLock<
    Option<carlio_os::persistent_log::PersistentLog<ahci::AhciBlockDevice>>,
> = IrqSpinLock::new(None);
#[cfg(target_os = "none")]
type KernelSwap = carlio_os::swap::SwapStore<'static, ahci::AhciBlockDevice>;
#[cfg(target_os = "none")]
static BOOT_SWAP: IrqSpinLock<Option<KernelSwap>> = IrqSpinLock::new(None);
#[cfg(target_os = "none")]
static mut HHDM_OFFSET: u64 = 0;
#[cfg(target_os = "none")]
static mut KERNEL_PHYSICAL_BASE: u64 = 0;
#[cfg(target_os = "none")]
static mut KERNEL_VIRTUAL_BASE: u64 = 0;
#[cfg(target_os = "none")]
static FRAME_ALLOCATOR: IrqSpinLock<PhysicalFrameAllocator<PHYSICAL_FRAME_BITMAP_WORDS>> =
    IrqSpinLock::new(PhysicalFrameAllocator::new());
#[cfg(target_os = "none")]
static PROCESS_SCHEDULER: Scheduler<64, MAX_BOOT_CPUS, RuntimeProcessVm> = Scheduler::new();
#[cfg(target_os = "none")]
// ELF parsing, VM ownership and mounted-filesystem I/O can nest under execve.
static PROCESS_KERNEL_STACKS: KernelStackPool<64, PROCESS_KERNEL_STACK_SIZE> =
    KernelStackPool::new();
#[cfg(target_os = "none")]
static NETWORK_SOCKETS: IrqSpinLock<carlio_os::net::SocketTable<128>> =
    IrqSpinLock::new(carlio_os::net::SocketTable::new());
#[cfg(target_os = "none")]
const PROCESS_VFS_CAPACITY: usize = 40;
#[cfg(target_os = "none")]
static PROCESS_VFS: IrqSpinLock<Vfs<PROCESS_VFS_CAPACITY>> = IrqSpinLock::new(Vfs::new());
#[cfg(target_os = "none")]
// Mounted ELF files are independent of the inline overlay's file size.
const EXEC_FILE_MAX: usize = 512 * 1024;
#[cfg(target_os = "none")]
static ELF_LOAD_SCRATCH: IrqSpinLock<[u8; EXEC_FILE_MAX]> = IrqSpinLock::new([0; EXEC_FILE_MAX]);
#[cfg(target_os = "none")]
static ELF_INTERPRETER_SCRATCH: IrqSpinLock<[u8; EXEC_FILE_MAX]> =
    IrqSpinLock::new([0; EXEC_FILE_MAX]);
#[cfg(target_os = "none")]
static ROOTFS_COPY_SCRATCH: IrqSpinLock<[u8; carlio_os::vfs::VFS_INLINE_FILE_MAX]> =
    IrqSpinLock::new([0; carlio_os::vfs::VFS_INLINE_FILE_MAX]);
#[cfg(target_os = "none")]
static OPEN_FILES: IrqSpinLock<carlio_os::open_files::OpenFileTable<128>> =
    IrqSpinLock::new(carlio_os::open_files::OpenFileTable::new());
#[cfg(target_os = "none")]
static AHCI_BLOCK_DEVICE: IrqSpinLock<Option<ahci::AhciBlockDevice>> = IrqSpinLock::new(None);
#[cfg(target_os = "none")]
enum BootRootfs {
    Ext4(carlio_os::ext4::Ext4Volume<carlio_os::block::BlockRegion<ahci::AhciBlockDevice>>),
    Fat32(carlio_os::fat32::Fat32Volume<carlio_os::block::BlockRegion<ahci::AhciBlockDevice>>),
}
#[cfg(target_os = "none")]
impl carlio_os::vfs::ReadOnlyMountBackend for BootRootfs {
    fn filesystem_type(&self) -> carlio_os::vfs::FilesystemType {
        match self {
            Self::Ext4(_) => carlio_os::vfs::FilesystemType::Ext4,
            Self::Fat32(_) => carlio_os::vfs::FilesystemType::Fat32,
        }
    }
    fn read_file(
        &mut self,
        path: &str,
        output: &mut [u8],
    ) -> Result<usize, carlio_os::vfs::VfsError> {
        match self {
            Self::Ext4(volume) => {
                carlio_os::vfs::ReadOnlyMountBackend::read_file(volume, path, output)
            }
            Self::Fat32(volume) => {
                carlio_os::vfs::ReadOnlyMountBackend::read_file(volume, path, output)
            }
        }
    }
    fn read_file_range(
        &mut self,
        path: &str,
        offset: u64,
        output: &mut [u8],
    ) -> Result<usize, carlio_os::vfs::VfsError> {
        match self {
            Self::Ext4(volume) => {
                carlio_os::vfs::ReadOnlyMountBackend::read_file_range(volume, path, offset, output)
            }
            Self::Fat32(volume) => {
                carlio_os::vfs::ReadOnlyMountBackend::read_file_range(volume, path, offset, output)
            }
        }
    }
    fn metadata(
        &mut self,
        path: &str,
    ) -> Result<carlio_os::vfs::MountMetadata, carlio_os::vfs::VfsError> {
        match self {
            Self::Ext4(volume) => carlio_os::vfs::ReadOnlyMountBackend::metadata(volume, path),
            Self::Fat32(volume) => carlio_os::vfs::ReadOnlyMountBackend::metadata(volume, path),
        }
    }
}
#[cfg(target_os = "none")]
static ROOTFS_BACKEND: IrqSpinLock<Option<BootRootfs>> = IrqSpinLock::new(None);
#[cfg(target_os = "none")]
static VFS_MUTATION_ACTIVE: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "none")]
const VFS_SNAPSHOT_MAX: usize = Vfs::<PROCESS_VFS_CAPACITY>::SNAPSHOT_MAX;
#[cfg(target_os = "none")]
type OverlayStore =
    carlio_os::vfs_store::VfsStore<carlio_os::block::BlockRegion<ahci::AhciBlockDevice>>;
#[cfg(target_os = "none")]
static VFS_STORE: IrqSpinLock<Option<OverlayStore>> = IrqSpinLock::new(None);
#[cfg(target_os = "none")]
static VFS_STORE_FAILED: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "none")]
static VFS_BEFORE: IrqSpinLock<[u8; VFS_SNAPSHOT_MAX]> = IrqSpinLock::new([0; VFS_SNAPSHOT_MAX]);
#[cfg(target_os = "none")]
static VFS_AFTER: IrqSpinLock<[u8; VFS_SNAPSHOT_MAX]> = IrqSpinLock::new([0; VFS_SNAPSHOT_MAX]);

#[cfg(target_os = "none")]
struct VfsMutationGuard {
    before: Option<usize>,
    committed: core::cell::Cell<bool>,
}

#[cfg(target_os = "none")]
impl VfsMutationGuard {
    fn acquire() -> Result<Self, u16> {
        use carlio_os::linux_abi::{EAGAIN, EIO};
        VFS_MUTATION_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| EAGAIN)?;
        let mut guard = Self {
            before: None,
            committed: core::cell::Cell::new(false),
        };
        if VFS_STORE_FAILED.load(Ordering::Acquire) {
            return Err(EIO);
        }
        if VFS_STORE.lock().is_some() {
            guard.before = Some(
                PROCESS_VFS
                    .lock()
                    .encode_snapshot(&mut *VFS_BEFORE.lock())
                    .map_err(vfs_errno)?,
            );
        }
        Ok(guard)
    }
}

#[cfg(target_os = "none")]
impl Drop for VfsMutationGuard {
    fn drop(&mut self) {
        if let Some(length) = self.before.filter(|_| !self.committed.get()) {
            let mut vfs = PROCESS_VFS.lock();
            let before = VFS_BEFORE.lock();
            if vfs.restore_overlay(&before[..length]).is_err() {
                VFS_STORE_FAILED.store(true, Ordering::Release);
            }
        }
        VFS_MUTATION_ACTIVE.store(false, Ordering::Release);
    }
}
#[cfg(target_os = "none")]
static PROCESS_PROBE_IDS: [AtomicU64; 3] =
    [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];
#[cfg(target_os = "none")]
static PROCESS_PROBE_SWITCHES: AtomicU64 = AtomicU64::new(0);
#[cfg(target_os = "none")]
static PROCESS_PROBE_CR3_SWITCHES: AtomicU64 = AtomicU64::new(0);
#[cfg(target_os = "none")]
static PROCESS_KERNEL_CR3: AtomicU64 = AtomicU64::new(0);
#[cfg(target_os = "none")]
static PROCESS_REAP_BARRIER: ReapBarrier<MAX_BOOT_CPUS> = ReapBarrier::new();
#[cfg(target_os = "none")]
static AP_CPL3_CPU_MASK: AtomicUsize = AtomicUsize::new(0);
#[cfg(target_os = "none")]
static AP_CPL3_RUNNING_MASK: AtomicUsize = AtomicUsize::new(0);
#[cfg(target_os = "none")]
static AP_CPL3_IDLE_MASK: AtomicUsize = AtomicUsize::new(0);
#[cfg(target_os = "none")]
static AP_CPL3_OVERLAP_PASSED: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "none")]
static AP_CPL3_STOP: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "none")]
static AP_VM_STRESS_MUNMAPS: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];
#[cfg(all(target_os = "none", not(feature = "smp-stress-test")))]
const AP_VM_STRESS_TARGET: usize = 512;
#[cfg(all(target_os = "none", feature = "smp-stress-test"))]
const AP_VM_STRESS_TARGET: usize = 8192;
#[cfg(target_os = "none")]
static PROCESS_PROBE_FAULTS: AtomicUsize = AtomicUsize::new(0);
#[cfg(target_os = "none")]
static PROCESS_PROBE_STARTED: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "none")]
static PROCESS_PROBE_DONE: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "none")]
static LINUX_GETPID_PROBE_PASSED: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "none")]
static LINUX_VFS_DENIAL_PROBE_PASSED: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "none")]
static LINUX_VFS_DENIAL_RESULT: AtomicU64 = AtomicU64::new(0);
#[cfg(target_os = "none")]
static LINUX_CREDENTIAL_PROBE_MASK: AtomicUsize = AtomicUsize::new(0);
#[cfg(target_os = "none")]
static LINUX_MEMORY_PROBE_MASK: AtomicUsize = AtomicUsize::new(0);
#[cfg(target_os = "none")]
static LINUX_MUNMAP_RESULT: AtomicU64 = AtomicU64::new(0);
#[cfg(target_os = "none")]
static AP_PROCESS_SCHEDULING: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "none")]
static PAGE_TABLE_PROBE_PENDING: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "none")]
const PROCESS_VM_PAGE_CAPACITY: usize = 128;
#[cfg(target_os = "none")]
const PROCESS_VM_REGION_CAPACITY: usize = 64;
#[cfg(target_os = "none")]
type RuntimeProcessVm = carlio_os::vma::ProcessVm<
    BootPageTableAccess,
    PROCESS_VM_PAGE_CAPACITY,
    PROCESS_VM_REGION_CAPACITY,
>;
#[cfg(target_os = "none")]
type ProcessVmSlot = OwnedProcessResource<RuntimeProcessVm>;

#[cfg(target_os = "none")]
static mut KERNEL_HEAP_BOOT_REGION: KernelHeapBootRegion = KernelHeapBootRegion {
    _bytes: [0; 64 * 1024],
};
#[cfg(target_os = "none")]
static KERNEL_HEAP: IrqSpinLock<KernelHeapSet<256>> = IrqSpinLock::new(KernelHeapSet::new());
#[cfg(target_os = "none")]
#[repr(align(4096))]
struct KernelHeapBootRegion {
    _bytes: [u8; 64 * 1024],
}
#[cfg(target_os = "none")]
#[repr(C, align(16))]
struct ApBootStacks([[u8; AP_STACK_SIZE]; MAX_BOOT_CPUS]);
#[cfg(target_os = "none")]
#[no_mangle]
static mut CARLIO_AP_BOOT_STACKS: ApBootStacks = ApBootStacks([[0; AP_STACK_SIZE]; MAX_BOOT_CPUS]);
#[cfg(target_os = "none")]
static AP_ONLINE_COUNT: AtomicUsize = AtomicUsize::new(1);
#[cfg(target_os = "none")]
static SMP_DETECTED_COUNT: AtomicUsize = AtomicUsize::new(1);
#[cfg(target_os = "none")]
static SMP_BSP_LAPIC_ID: AtomicU32 = AtomicU32::new(u32::MAX);
#[cfg(target_os = "none")]
static FRAMEBUFFER_ACTIVE: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "none")]
const TASK_IDLE: u32 = 0;
#[cfg(target_os = "none")]
const TASK_QUEUED: u32 = 1;
#[cfg(target_os = "none")]
const TASK_RUNNING: u32 = 2;
#[cfg(target_os = "none")]
const TASK_COMPLETE: u32 = 3;
#[cfg(target_os = "none")]
const TASK_SMP_PROBE: u32 = 1;

#[cfg(target_os = "none")]
struct CpuTaskMailbox {
    state: AtomicU32,
    kind: AtomicU32,
    argument: AtomicU64,
    result: AtomicU64,
}
#[cfg(target_os = "none")]
impl CpuTaskMailbox {
    const fn new() -> Self {
        Self {
            state: AtomicU32::new(TASK_IDLE),
            kind: AtomicU32::new(0),
            argument: AtomicU64::new(0),
            result: AtomicU64::new(0),
        }
    }
}

#[cfg(target_os = "none")]
struct BootCpuLocal {
    processor_id: AtomicU32,
    lapic_id: AtomicU32,
    online: AtomicU32,
    task: CpuTaskMailbox,
}
#[cfg(target_os = "none")]
impl BootCpuLocal {
    const fn new() -> Self {
        Self {
            processor_id: AtomicU32::new(u32::MAX),
            lapic_id: AtomicU32::new(u32::MAX),
            online: AtomicU32::new(0),
            task: CpuTaskMailbox::new(),
        }
    }
}
#[cfg(target_os = "none")]
static BOOT_CPU_LOCALS: [BootCpuLocal; MAX_BOOT_CPUS] =
    [const { BootCpuLocal::new() }; MAX_BOOT_CPUS];

#[cfg(target_os = "none")]
#[repr(C)]
struct LimineSmpInfo {
    processor_id: u32,
    lapic_id: u32,
    reserved: u64,
    goto_address: usize,
    extra_argument: u64,
}

#[cfg(target_os = "none")]
#[repr(C)]
struct LimineSmpResponse {
    revision: u64,
    flags: u32,
    bsp_lapic_id: u32,
    cpu_count: u64,
    cpus: *mut *mut LimineSmpInfo,
}

#[cfg(target_os = "none")]
#[repr(C)]
struct LimineSmpRequest {
    id: [u64; 4],
    revision: u64,
    response: *mut LimineSmpResponse,
    flags: u64,
}

#[cfg(target_os = "none")]
#[repr(C)]
struct LimineFramebuffer {
    address: *mut u8,
    width: u64,
    height: u64,
    pitch: u64,
    bits_per_pixel: u16,
    memory_model: u8,
    red_mask_size: u8,
    red_mask_shift: u8,
    green_mask_size: u8,
    green_mask_shift: u8,
    blue_mask_size: u8,
    blue_mask_shift: u8,
    unused: [u8; 7],
    edid_size: u64,
    edid: *mut u8,
    mode_count: u64,
    modes: *mut *mut LimineVideoMode,
}

#[cfg(target_os = "none")]
#[repr(C)]
struct LimineVideoMode {
    pitch: u64,
    width: u64,
    height: u64,
    bits_per_pixel: u16,
    memory_model: u8,
    red_mask_size: u8,
    red_mask_shift: u8,
    green_mask_size: u8,
    green_mask_shift: u8,
    blue_mask_size: u8,
    blue_mask_shift: u8,
}

#[cfg(target_os = "none")]
#[repr(C)]
struct LimineFramebufferResponse {
    revision: u64,
    framebuffer_count: u64,
    framebuffers: *mut *mut LimineFramebuffer,
}

#[cfg(target_os = "none")]
#[repr(C)]
struct LimineFramebufferRequest {
    id: [u64; 4],
    revision: u64,
    response: *mut LimineFramebufferResponse,
}

#[cfg(target_os = "none")]
struct BootPageTableAccess {
    hhdm: u64,
}

#[cfg(target_os = "none")]
impl PageTableAccess for BootPageTableAccess {
    fn read_entry(&self, table: u64, index: usize) -> Result<u64, FrameError> {
        if index >= 512 {
            return Err(FrameError::CorruptTable);
        }
        let address = self
            .hhdm
            .checked_add(table)
            .ok_or(FrameError::InvalidPhysicalAddress)?;
        let slot = (address as *const u64).wrapping_add(index);
        Ok(unsafe { core::ptr::read_volatile(slot) })
    }
    fn write_entry(&mut self, table: u64, index: usize, value: u64) -> Result<(), FrameError> {
        if index >= 512 {
            return Err(FrameError::CorruptTable);
        }
        let address = self
            .hhdm
            .checked_add(table)
            .ok_or(FrameError::InvalidPhysicalAddress)?;
        let slot = (address as *mut u64).wrapping_add(index);
        unsafe { core::ptr::write_volatile(slot, value) };
        Ok(())
    }
    fn allocate_table(&mut self) -> Result<u64, FrameError> {
        let frame = FRAME_ALLOCATOR
            .lock()
            .allocate()
            .ok_or(FrameError::TableAllocationFailed)?;
        let address = match self.hhdm.checked_add(frame) {
            Some(address) => address,
            None => {
                let _ = FRAME_ALLOCATOR.lock().free(frame);
                return Err(FrameError::InvalidPhysicalAddress);
            }
        };
        unsafe { core::ptr::write_bytes(address as *mut u8, 0, 4096) };
        Ok(frame)
    }
    fn release_table(&mut self, physical: u64) -> Result<(), FrameError> {
        FRAME_ALLOCATOR.lock().free(physical)
    }
    fn invalidate_page(&mut self, virtual_address: u64) {
        if lapic::is_initialized() {
            carlio_os::memory::TlbShootdown::invalidate_all(
                &lapic::LapicShootdown,
                0,
                virtual_address,
            );
        } else {
            unsafe {
                asm!("invlpg [{}]", in(reg) virtual_address, options(nostack, preserves_flags))
            };
        }
    }
}

#[cfg(target_os = "none")]
#[cfg(target_os = "none")]
struct ElfAddressSpace<const PAGES: usize>(OwnedAddressSpace<BootPageTableAccess, PAGES>);

#[cfg(target_os = "none")]
impl<const PAGES: usize> ElfAddressSpace<PAGES> {
    fn new(kernel_root: u64) -> Result<Self, FrameError> {
        let hhdm = unsafe { core::ptr::addr_of!(HHDM_OFFSET).read_volatile() };
        OwnedAddressSpace::new(BootPageTableAccess { hhdm }, kernel_root).map(Self)
    }
}

#[cfg(target_os = "none")]
impl<const PAGES: usize> carlio_os::elf::ElfLoadTarget for ElfAddressSpace<PAGES> {
    type Error = FrameError;

    fn map_zeroed_page(&mut self, page: u64) -> Result<(), Self::Error> {
        self.0.map_zeroed(
            page,
            1 | PAGE_FLAG_USER
                | carlio_os::memory::PAGE_FLAG_WRITABLE
                | carlio_os::memory::PAGE_FLAG_NO_EXECUTE,
        )?;
        Ok(())
    }

    fn write(&mut self, mut address: u64, mut bytes: &[u8]) -> Result<(), Self::Error> {
        while !bytes.is_empty() {
            let mapping = match self.0.mapping(address & !0xfff) {
                Ok(mapping) => mapping,
                Err(error) => {
                    write("Runtime: stack/ELF sem mapeamento para VA 0x");
                    write_hex_u64(address);
                    write(".\r\n");
                    return Err(error);
                }
            };
            let page_offset = (address & 0xfff) as usize;
            let count = bytes.len().min(4096 - page_offset);
            let hhdm = unsafe { core::ptr::addr_of!(HHDM_OFFSET).read_volatile() };
            let physical = mapping.physical & !0xfff;
            let destination =
                hhdm.checked_add(physical)
                    .and_then(|base| base.checked_add(page_offset as u64))
                    .ok_or(FrameError::InvalidPhysicalAddress)? as *mut u8;
            unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), destination, count) };
            address = address
                .checked_add(count as u64)
                .ok_or(FrameError::InvalidVirtualAddress)?;
            bytes = &bytes[count..];
        }
        Ok(())
    }

    fn protect_page(&mut self, page: u64, flags: u32) -> Result<(), Self::Error> {
        let mut pte = 1 | PAGE_FLAG_USER;
        if flags & carlio_os::elf::PF_W != 0 {
            pte |= carlio_os::memory::PAGE_FLAG_WRITABLE;
        }
        if flags & carlio_os::elf::PF_X == 0 {
            pte |= carlio_os::memory::PAGE_FLAG_NO_EXECUTE;
        }
        self.0.protect(page, pte)
    }

    fn unmap_page(&mut self, page: u64) -> Result<(), Self::Error> {
        self.0.unmap(page)
    }
}

#[cfg(target_os = "none")]
global_asm!(
    r#"
    .text
    .global carlio_ap_trampoline
carlio_ap_trampoline:
    cli
    movq 24(%rdi), %rax
    cmpq $64, %rax
    jae 2f
    leaq CARLIO_AP_BOOT_STACKS(%rip), %rsp
    shlq $14, %rax
    addq $16384, %rax
    addq %rax, %rsp
    andq $-16, %rsp
    xorq %rbp, %rbp
    call carlio_ap_main
2:
    cli
1:
    pause
    jmp 1b
    "#,
    options(att_syntax)
);

#[cfg(target_os = "none")]
unsafe extern "C" {
    fn carlio_ap_trampoline();
}

#[cfg(target_os = "none")]
#[no_mangle]
extern "C" fn carlio_ap_main(info: *mut LimineSmpInfo) -> ! {
    let index = if !info.is_null() {
        let info = unsafe { &*info };
        let index = info.extra_argument as usize;
        if index < MAX_BOOT_CPUS {
            if !unsafe { idt::load_shared_kernel_gdt(index) } {
                loop {
                    unsafe { asm!("cli; hlt", options(nomem, nostack)) };
                }
            }
            unsafe { idt::load_shared_idt() };
            user_fpu::enable();
            if unsafe { lapic::initialize(lapic::MMIO_VIRTUAL_BASE) }.is_err() {
                loop {
                    unsafe { asm!("cli; hlt", options(nomem, nostack)) };
                }
            }
            unsafe { lapic::start_scheduler_timer() };
            let local = &BOOT_CPU_LOCALS[index];
            local
                .processor_id
                .store(info.processor_id, Ordering::Relaxed);
            local.lapic_id.store(info.lapic_id, Ordering::Relaxed);
            local.online.store(1, Ordering::Release);
            AP_ONLINE_COUNT.fetch_add(1, Ordering::Release);
            unsafe { asm!("sti", options(nomem, nostack)) };
        }
        index
    } else {
        usize::MAX
    };
    let mut scheduler_timer_started = false;
    loop {
        if !info.is_null() && index < MAX_BOOT_CPUS {
            if ap_acknowledge_process_quiescence(index) {
                continue;
            }
            let mailbox = &BOOT_CPU_LOCALS[index].task;
            if mailbox
                .state
                .compare_exchange(
                    TASK_QUEUED,
                    TASK_RUNNING,
                    Ordering::Acquire,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                let value = match mailbox.kind.load(Ordering::Relaxed) {
                    TASK_SMP_PROBE => smp_probe_work(mailbox.argument.load(Ordering::Relaxed)),
                    _ => 0,
                };
                mailbox.result.store(value, Ordering::Relaxed);
                mailbox.state.store(TASK_COMPLETE, Ordering::Release);
                continue;
            }
            if AP_PROCESS_SCHEDULING.load(Ordering::Acquire) {
                if PROCESS_PROBE_STARTED.load(Ordering::Acquire)
                    && !scheduler_has_current_process(index)
                {
                    AP_CPL3_IDLE_MASK.fetch_or(1usize << index, Ordering::AcqRel);
                }
                if !scheduler_timer_started {
                    unsafe { lapic::start_scheduler_timer() };
                    scheduler_timer_started = true;
                }
                if let Some(process) = schedule_process_on_cpu(index).ok().flatten() {
                    if process.interrupt_frame != 0
                        && process.kernel_stack_top != 0
                        && activate_scheduled_process(index, process)
                    {
                        if is_secondary_cpu(index) && is_ap_cpl3_probe_id(process.id) {
                            mark_ap_cpl3_running(index);
                        }
                        unsafe {
                            idt::enter_user_frame(
                                process.interrupt_frame as *mut idt::UserInterruptFrame,
                            )
                        };
                    }
                }
            }
        }
        core::hint::spin_loop();
    }
}

#[cfg(target_os = "none")]
#[no_mangle]
pub(crate) extern "C" fn carlio_ap_idle_resume() -> ! {
    let cpu = current_scheduler_cpu();
    let stack_top = unsafe {
        core::ptr::addr_of_mut!(CARLIO_AP_BOOT_STACKS.0[cpu])
            .cast::<u8>()
            .add(AP_STACK_SIZE) as u64
    };
    unsafe {
        asm!(
            "mov rsp, {stack_top}",
            "and rsp, -16",
            "mov edi, {cpu:e}",
            "call carlio_ap_scheduler_idle",
            stack_top = in(reg) stack_top,
            cpu = in(reg) cpu,
            options(noreturn)
        );
    }
}

#[cfg(target_os = "none")]
#[no_mangle]
extern "C" fn carlio_ap_scheduler_idle(cpu: usize) -> ! {
    let stack_top = unsafe {
        core::ptr::addr_of_mut!(CARLIO_AP_BOOT_STACKS.0[cpu])
            .cast::<u8>()
            .add(AP_STACK_SIZE) as u64
    };
    let kernel_root = PROCESS_KERNEL_CR3.load(Ordering::Acquire);
    if kernel_root != 0
        && unsafe { idt::activate_cpu_process_address_space(cpu, kernel_root, stack_top) }
        && PROCESS_PROBE_STARTED.load(Ordering::Acquire)
    {
        AP_CPL3_IDLE_MASK.fetch_or(1usize << cpu, Ordering::AcqRel);
    }
    unsafe { asm!("sti", options(nomem, nostack)) };
    loop {
        if ap_acknowledge_process_quiescence(cpu) {
            continue;
        }
        if let Some(process) = schedule_process_on_cpu(cpu).ok().flatten() {
            if process.interrupt_frame != 0
                && process.kernel_stack_top != 0
                && activate_scheduled_process(cpu, process)
            {
                if is_secondary_cpu(cpu) && is_ap_cpl3_probe_id(process.id) {
                    mark_ap_cpl3_running(cpu);
                }
                unsafe {
                    idt::enter_user_frame(process.interrupt_frame as *mut idt::UserInterruptFrame)
                };
            }
        }
        core::hint::spin_loop();
    }
}

#[cfg(target_os = "none")]
fn ap_acknowledge_process_quiescence(cpu: usize) -> bool {
    let Some(epoch) = PROCESS_REAP_BARRIER.requested() else {
        return false;
    };
    if cpu >= MAX_BOOT_CPUS {
        return false;
    }
    let stack_top = unsafe {
        core::ptr::addr_of_mut!(CARLIO_AP_BOOT_STACKS.0[cpu])
            .cast::<u8>()
            .add(AP_STACK_SIZE) as u64
    };
    let kernel_root = PROCESS_KERNEL_CR3.load(Ordering::Acquire);
    if kernel_root == 0
        || !unsafe { idt::activate_cpu_process_address_space(cpu, kernel_root, stack_top) }
    {
        loop {
            unsafe { asm!("cli; hlt", options(nomem, nostack)) };
        }
    }
    let current_root: u64;
    // SAFETY: CR3 é lido em long mode para validar que esta CPU já está na raiz do kernel.
    unsafe { asm!("mov {}, cr3", out(reg) current_root, options(nomem, nostack, preserves_flags)) };
    let bit = 1usize << cpu;
    if current_root & 0x000f_ffff_ffff_f000 != kernel_root
        || AP_CPL3_RUNNING_MASK.load(Ordering::Acquire) & bit != 0
    {
        return false;
    }
    AP_CPL3_IDLE_MASK.fetch_or(1usize << cpu, Ordering::AcqRel);
    if !PROCESS_REAP_BARRIER.acknowledge(cpu, epoch) {
        return false;
    }
    while PROCESS_REAP_BARRIER.requested() == Some(epoch) {
        core::hint::spin_loop();
    }
    true
}

#[cfg(target_os = "none")]
fn smp_probe_work(seed: u64) -> u64 {
    let mut value = seed;
    for step in 0..32_768u64 {
        value = value.rotate_left(7) ^ step.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
    value
}

#[repr(C)]
struct LimineKernelAddressResponse {
    revision: u64,
    physical_base: u64,
    virtual_base: u64,
}

#[repr(C)]
struct LimineKernelAddressRequest {
    id: [u64; 4],
    revision: u64,
    response: *mut LimineKernelAddressResponse,
}

#[repr(C)]
struct LimineHhdmResponse {
    revision: u64,
    offset: u64,
}

#[repr(C)]
struct LimineHhdmRequest {
    id: [u64; 4],
    revision: u64,
    response: *mut LimineHhdmResponse,
}

#[repr(C)]
struct LimineMemmapEntry {
    base: u64,
    length: u64,
    kind: u64,
}

#[repr(C)]
struct LimineMemmapResponse {
    revision: u64,
    entry_count: u64,
    entries: *mut *mut LimineMemmapEntry,
}

#[repr(C)]
struct LimineMemmapRequest {
    id: [u64; 4],
    revision: u64,
    response: *mut LimineMemmapResponse,
}

#[used]
#[link_section = ".limine_requests"]
static mut HHDM_REQUEST: LimineHhdmRequest = LimineHhdmRequest {
    id: [
        0xc7b1_dd30_df4c_8b88,
        0x0a82_e883_a194_f07b,
        0x48dc_f1cb_8ad2_b852,
        0x6398_4e95_9a98_244b,
    ],
    revision: 0,
    response: core::ptr::null_mut(),
};

#[used]
#[link_section = ".limine_requests"]
static mut MEMMAP_REQUEST: LimineMemmapRequest = LimineMemmapRequest {
    id: [
        0xc7b1_dd30_df4c_8b88,
        0x0a82_e883_a194_f07b,
        0x67cf_3d9d_378a_806f,
        0xe304_acdf_c50c_3c62,
    ],
    revision: 0,
    response: core::ptr::null_mut(),
};

#[used]
#[link_section = ".limine_requests"]
static mut KERNEL_ADDRESS_REQUEST: LimineKernelAddressRequest = LimineKernelAddressRequest {
    id: [
        0xc7b1_dd30_df4c_8b88,
        0x0a82_e883_a194_f07b,
        0x71ba_7686_3cc5_5f63,
        0xb264_4a48_c516_a487,
    ],
    revision: 0,
    response: core::ptr::null_mut(),
};

#[cfg(target_os = "none")]
#[used]
#[link_section = ".limine_requests"]
static mut SMP_REQUEST: LimineSmpRequest = LimineSmpRequest {
    id: [
        0xc7b1_dd30_df4c_8b88,
        0x0a82_e883_a194_f07b,
        0x95a6_7b81_9a1b_857e,
        0xa0b6_1b72_3b6a_73e0,
    ],
    revision: 0,
    response: core::ptr::null_mut(),
    flags: 0,
};

#[cfg(target_os = "none")]
#[used]
#[link_section = ".limine_requests"]
static mut FRAMEBUFFER_REQUEST: LimineFramebufferRequest = LimineFramebufferRequest {
    id: [
        0xc7b1_dd30_df4c_8b88,
        0x0a82_e883_a194_f07b,
        0x9d58_27dc_d881_dd75,
        0xa314_8604_f6fa_b11b,
    ],
    revision: 1,
    response: core::ptr::null_mut(),
};

#[cfg(target_os = "none")]
#[path = "arch/x86_64/ahci.rs"]
mod ahci;
#[cfg(target_os = "none")]
#[path = "arch/x86_64/e1000.rs"]
mod e1000;
#[cfg(target_os = "none")]
#[path = "arch/x86_64/entropy.rs"]
mod entropy;
#[cfg(target_os = "none")]
#[path = "arch/x86_64/idt.rs"]
mod idt;
#[cfg(target_os = "none")]
#[path = "arch/x86_64/lapic.rs"]
mod lapic;
#[cfg(target_os = "none")]
#[path = "arch/x86_64/pci.rs"]
mod pci_device;
#[cfg(target_os = "none")]
#[path = "arch/x86_64/user_fpu.rs"]
mod user_fpu;

#[used]
#[link_section = ".limine_requests_start"]
static REQUESTS_START: [u64; 2] = [0xf6b8_f4b3_9de7_d1ae, 0xfab9_1a69_40fc_b9cf];

#[used]
#[link_section = ".limine_requests"]
static BASE_REVISION: [u64; 3] = [0xf956_2b2d_5c95_a6c8, 0x6a7b_3849_4453_6bdc, 3];

#[used]
#[link_section = ".limine_requests_end"]
static REQUESTS_END: [u64; 2] = [0xadc0_e053_1bb1_0d03, 0x9572_709f_3176_4c62];

#[no_mangle]
#[cfg(target_os = "none")]
pub extern "C" fn _start() -> ! {
    serial_init();
    unsafe { idt::install() };
    user_fpu::enable();
    startup_screen();
    boot_progress(5, "kernel carregado");
    video_boot_probe();
    boot_progress(12, "vídeo/console");
    write("IDT: exceções 0-31 registradas; IRQs externos permanecem desativados.\r\n");
    if idt::ring3_descriptors_ready() {
        write("Ring 3: GDT, TSS, RSP0 e seletores de usuário configurados; CPL continua 0.\r\n");
    } else {
        write("Ring 3: configuração GDT/TSS inválida.\r\n");
        record_error(KernelError::DeviceInitFailed);
    }
    write("Aviso: terminal de diagnóstico em ring 0; não é uma fronteira de segurança.\r\n");
    report_memory_map();
    boot_progress(25, "memória física e HHDM");
    let hhdm = unsafe { core::ptr::addr_of!(HHDM_OFFSET).read_volatile() };
    if prepare_lapic_mmio(hhdm) && unsafe { lapic::initialize(lapic::MMIO_VIRTUAL_BASE) }.is_ok() {
        write("LAPIC xAPIC ativo no BSP; vetor de shootdown remoto 0xF1 instalado.\r\n");
        start_secondary_processors();
    } else {
        write("LAPIC indisponível; SMP e shootdown remoto desativados por segurança.\r\n");
    }
    lapic::set_online_aps(AP_ONLINE_COUNT.load(Ordering::Acquire).saturating_sub(1));
    run_smp_parallel_probe();
    boot_progress(45, "inicialização SMP");
    kernel_heap_boot_probe();
    boot_progress(58, "heap do kernel");
    enumerate_pci();
    boot_progress(70, "detecção de hardware");
    initialize_vfs_mounts();
    initialize_persistent_storage();
    boot_progress(82, "VFS e rootfs");
    unsafe { idt::install_timer_frame_switch(timer_process_switch) };
    if PAGE_TABLE_PROBE_PENDING.swap(false, Ordering::AcqRel) {
        let hhdm = unsafe { core::ptr::addr_of!(HHDM_OFFSET).read_volatile() };
        page_table_boot_probe(hhdm);
    }
    boot_progress(92, "VM e processos");
    // O probe CPL3 pode retornar ao kernel com IF desabilitado. Reativar e
    // reinicializar o PIT após a prova para que o watchdog também cubra o shell.
    unsafe { idt::start_watchdog() };
    if lapic::is_initialized() {
        unsafe { lapic::start_scheduler_timer() };
    }
    AP_PROCESS_SCHEDULING.store(true, Ordering::Release);
    boot_progress(100, "shell disponível");
    write("Watchdog PIT ativo: prazo de 5 s sem progresso.\r\n");
    write("Digite 'ajuda' para ver os comandos.\r\n\r\n");
    shell()
}

#[cfg(target_os = "none")]
fn start_secondary_processors() {
    let response = unsafe { core::ptr::addr_of!(SMP_REQUEST.response).read_volatile() };
    if response.is_null() {
        write("SMP: resposta do Limine ausente; somente BSP ativo.\r\n");
        return;
    }
    let cpu_count = unsafe { (*response).cpu_count };
    let cpus = unsafe { (*response).cpus };
    let bsp_lapic_id = unsafe { (*response).bsp_lapic_id };
    if cpus.is_null() || cpu_count == 0 {
        write("SMP: inventário de CPUs inválido; somente BSP ativo.\r\n");
        record_error(KernelError::BootInfoMissing);
        return;
    }

    let limit = cpu_count.min(MAX_BOOT_CPUS as u64) as usize;
    SMP_DETECTED_COUNT.store(limit, Ordering::Release);
    SMP_BSP_LAPIC_ID.store(bsp_lapic_id, Ordering::Release);
    let trampoline = carlio_ap_trampoline as *const () as usize;
    let mut requested = 0usize;
    for index in 0..limit {
        let info = unsafe { cpus.add(index).read_volatile() };
        if info.is_null() {
            continue;
        }
        let processor_id = unsafe { (*info).processor_id };
        let lapic_id = unsafe { (*info).lapic_id };
        if lapic_id == bsp_lapic_id {
            let local = &BOOT_CPU_LOCALS[index];
            local.processor_id.store(processor_id, Ordering::Relaxed);
            local.lapic_id.store(lapic_id, Ordering::Relaxed);
            local.online.store(1, Ordering::Release);
            let _ = unsafe { idt::load_shared_kernel_gdt(index) };
            continue;
        }
        unsafe {
            core::ptr::addr_of_mut!((*info).extra_argument).write(index as u64);
            AtomicUsize::from_ptr(core::ptr::addr_of_mut!((*info).goto_address))
                .store(trampoline, Ordering::Release);
        }
        requested += 1;
    }

    let mut spins = 0usize;
    while AP_ONLINE_COUNT.load(Ordering::Acquire) < requested + 1 && spins < 10_000_000 {
        core::hint::spin_loop();
        spins += 1;
    }
    let online = AP_ONLINE_COUNT.load(Ordering::Acquire);
    write("SMP Limine: CPUs detectadas=");
    write_u32(cpu_count.min(u32::MAX as u64) as u32);
    write(", APs iniciadas=");
    write_u32(online.saturating_sub(1).min(u32::MAX as usize) as u32);
    write(", aguardadas=");
    write_u32(requested.min(u32::MAX as usize) as u32);
    write("; APs aguardam despacho de tarefa, scheduler ainda desativado.\r\n");
}

/// Mapeia a página física local APIC como uncached antes de habilitar xAPIC.
#[cfg(target_os = "none")]
fn prepare_lapic_mmio(hhdm: u64) -> bool {
    let physical = match lapic::physical_base() {
        Ok(address) if address != 0 => address,
        _ => return false,
    };
    let root: u64;
    unsafe { asm!("mov {}, cr3", out(reg) root, options(nomem, nostack, preserves_flags)) };
    let root = root & 0x000f_ffff_ffff_f000;
    let mut tables = match PageTableManager::new(BootPageTableAccess { hhdm }, root) {
        Ok(manager) => manager,
        Err(_) => return false,
    };
    let mapped = match tables.mapping(lapic::MMIO_VIRTUAL_BASE) {
        Ok(mapping) => mapping.physical == physical && mapping.flags & 0x18 == 0x18,
        Err(FrameError::NotMapped) => tables.map(lapic::MMIO_VIRTUAL_BASE, physical, 0x1b).is_ok(), // P | RW | PWT | PCD: MMIO sem cache
        Err(_) => false,
    };
    mapped && lapic::set_mmio_physical_base(physical)
}

#[cfg(target_os = "none")]
fn run_smp_parallel_probe() {
    let cpu_count = SMP_DETECTED_COUNT.load(Ordering::Acquire);
    let bsp_lapic = SMP_BSP_LAPIC_ID.load(Ordering::Acquire);
    let mut targets = [usize::MAX; MAX_BOOT_CPUS];
    let mut target_count = 0usize;
    for index in 0..cpu_count {
        let local = &BOOT_CPU_LOCALS[index];
        if local.online.load(Ordering::Acquire) == 0
            || local.lapic_id.load(Ordering::Relaxed) == bsp_lapic
        {
            continue;
        }
        let seed = 0x4341_524c_494f_0000u64 | index as u64;
        local.task.argument.store(seed, Ordering::Relaxed);
        local.task.kind.store(TASK_SMP_PROBE, Ordering::Relaxed);
        local.task.state.store(TASK_QUEUED, Ordering::Release);
        targets[target_count] = index;
        target_count += 1;
    }

    let mut spins = 0usize;
    loop {
        let complete = targets[..target_count].iter().all(|&index| {
            BOOT_CPU_LOCALS[index].task.state.load(Ordering::Acquire) == TASK_COMPLETE
        });
        if complete || spins >= 10_000_000 {
            break;
        }
        spins += 1;
        core::hint::spin_loop();
    }

    let mut completed = 0usize;
    let mut valid = 0usize;
    for &index in &targets[..target_count] {
        let mailbox = &BOOT_CPU_LOCALS[index].task;
        if mailbox.state.load(Ordering::Acquire) == TASK_COMPLETE {
            completed += 1;
            let seed = 0x4341_524c_494f_0000u64 | index as u64;
            if mailbox.result.load(Ordering::Relaxed) == smp_probe_work(seed) {
                valid += 1;
            }
            mailbox.state.store(TASK_IDLE, Ordering::Release);
        }
    }
    write("SMP trabalho: CPUs AP=");
    write_u32(target_count.min(u32::MAX as usize) as u32);
    write(", tarefas concluídas=");
    write_u32(completed.min(u32::MAX as usize) as u32);
    write(", resultados válidos=");
    write_u32(valid.min(u32::MAX as usize) as u32);
    write("; despacho de prova concluído.\r\n");
    if completed != target_count || valid != target_count {
        record_error(KernelError::DeviceInitFailed);
    }
}

#[cfg(not(target_os = "none"))]
fn main() {}

#[cfg(all(target_os = "none", feature = "storage-test"))]
struct StorageCutDisk {
    disk: ahci::AhciBlockDevice,
    cut: bool,
    metadata_cut: bool,
}

#[cfg(all(target_os = "none", feature = "storage-test"))]
impl carlio_os::block::BlockDevice for StorageCutDisk {
    fn sector_count(&self) -> u64 {
        self.disk.sector_count()
    }

    fn read_sector(
        &mut self,
        lba: u64,
        out: &mut [u8; 512],
    ) -> Result<(), carlio_os::block::BlockError> {
        self.disk.read_sector(lba, out)
    }

    fn write_sector(
        &mut self,
        lba: u64,
        data: &[u8; 512],
    ) -> Result<(), carlio_os::block::BlockError> {
        if self.cut && lba >= 2048 {
            // Simulate a torn sector at the device boundary, then stop before
            // the log can flush or publish the new sequence.
            let mut torn = *data;
            if self.metadata_cut {
                let mut previous = [0; 512];
                self.disk.read_sector(lba, &mut previous)?;
                let first_change = data
                    .iter()
                    .zip(previous)
                    .position(|(&new, old)| new != old)
                    .ok_or(carlio_os::block::BlockError::Device)?;
                let half_start = (first_change / 256) * 256;
                torn[half_start..half_start + 256].fill(0);
            } else {
                torn[256..].fill(0);
            }
            self.disk.write_sector(lba, &torn)?;
            write("Armazenamento: setor parcial escrito; flush pendente.\r\n");
            loop {
                core::hint::spin_loop();
            }
        }
        self.disk.write_sector(lba, data)
    }

    fn flush(&mut self) -> Result<(), carlio_os::block::BlockError> {
        self.disk.flush()
    }
}

#[cfg(all(target_os = "none", feature = "storage-test"))]
fn filesystem_cut_probe(cut: bool) -> Option<()> {
    use carlio_os::block::BlockDevice;

    const PARTITION: u64 = 2048;
    const JOURNAL: u64 = 64;
    const PATH: &str = "/probe.bin";
    let mut disk = ahci::block_device().ok()?;
    let mut marker = [0; 512];
    disk.read_sector(0, &mut marker).ok()?;
    if &marker[..15] != b"CARLIO-FS-TEST1" || marker[16..].iter().any(|&byte| byte != 0) {
        return None;
    }
    let disk = StorageCutDisk {
        disk,
        cut,
        metadata_cut: false,
    };
    let before = [0x35; 512];
    let after = [0x6d; 512];
    let mut current = [0; 512];
    match marker[15] {
        1 => {
            let mut fs = carlio_os::ext4::Ext4Volume::mount(disk, PARTITION).ok()?;
            if cut {
                fs.read_path(PATH, &mut current)
                    .ok()
                    .filter(|&len| len == 512)?;
                if current == before {
                    fs.overwrite_file_sector(PATH, JOURNAL, &after).ok()?;
                }
                return None;
            }
            fs.recover_file_sector(PATH, JOURNAL)
                .ok()
                .filter(|&replayed| replayed)?;
            fs.read_path(PATH, &mut current)
                .ok()
                .filter(|&len| len == 512)?;
            fs.recover_file_sector(PATH, JOURNAL)
                .ok()
                .filter(|&replayed| !replayed)?;
        }
        2 => {
            let mut fs = carlio_os::fat32::Fat32Volume::mount(disk, PARTITION).ok()?;
            if cut {
                fs.read_path(PATH, &mut current)
                    .ok()
                    .filter(|&len| len == 512)?;
                if current == before {
                    fs.overwrite_file_sector(PATH, JOURNAL, &after).ok()?;
                }
                return None;
            }
            fs.recover_file_sector(PATH, JOURNAL)
                .ok()
                .filter(|&replayed| replayed)?;
            fs.read_path(PATH, &mut current)
                .ok()
                .filter(|&len| len == 512)?;
            fs.recover_file_sector(PATH, JOURNAL)
                .ok()
                .filter(|&replayed| !replayed)?;
        }
        _ => return None,
    }
    if current != after {
        return None;
    }
    write("Filesystem: redo após corte e segunda recuperação idempotente OK.\r\n");
    Some(())
}

#[cfg(all(target_os = "none", feature = "storage-test"))]
fn filesystem_metadata_probe(cut: bool) -> Option<()> {
    use carlio_os::block::BlockDevice;
    use carlio_os::vfs::ReadOnlyMountBackend;

    const PARTITION: u64 = 2048;
    const JOURNAL: u64 = 64;
    const PATH: &str = "/probe.bin";
    let mut disk = ahci::block_device().ok()?;
    let mut marker = [0; 512];
    disk.read_sector(0, &mut marker).ok()?;
    if &marker[..15] != b"CARLIO-FS-META1" || marker[16..].iter().any(|&byte| byte != 0) {
        return None;
    }
    let disk = StorageCutDisk {
        disk,
        cut,
        metadata_cut: true,
    };
    let mut content = [0; 512];
    match marker[15] {
        1 => {
            let mut fs = carlio_os::ext4::Ext4Volume::mount(disk, PARTITION).ok()?;
            if cut {
                (fs.metadata(PATH).ok()?.mode == 0o644).then_some(())?;
                fs.set_inode_mode(PATH, JOURNAL, 0o640).ok()?;
                return None;
            }
            fs.recover_inode_mode(PATH, JOURNAL).ok()?.then_some(())?;
            (fs.metadata(PATH).ok()?.mode == 0o640).then_some(())?;
            (fs.read_file(PATH, &mut content).ok()? == 512).then_some(())?;
            (!fs.recover_inode_mode(PATH, JOURNAL).ok()?).then_some(())?;
        }
        2 => {
            let mut fs = carlio_os::fat32::Fat32Volume::mount(disk, PARTITION).ok()?;
            if cut {
                (fs.metadata(PATH).ok()?.mode == 0o666).then_some(())?;
                fs.set_file_read_only(PATH, JOURNAL, true).ok()?;
                return None;
            }
            fs.recover_file_attributes(JOURNAL).ok()?.then_some(())?;
            (fs.metadata(PATH).ok()?.mode == 0o444).then_some(())?;
            (fs.read_file(PATH, &mut content).ok()? == 512).then_some(())?;
            (!fs.recover_file_attributes(JOURNAL).ok()?).then_some(())?;
        }
        _ => return None,
    }
    (content == [0x35; 512]).then_some(())?;
    write("Filesystem: metadados nativos recuperados e segunda recuperação idempotente OK.\r\n");
    Some(())
}

#[cfg(target_os = "none")]
fn shell() -> ! {
    let mut line = [0u8; INPUT_LIMIT];
    loop {
        flush_persistent_errors();
        write("carlio> ");
        let mut len = 0;
        let mut overflowed = false;
        loop {
            let byte = match serial_read() {
                Ok(byte) => byte,
                Err(_) => continue,
            };
            match byte {
                b'\r' | b'\n' => {
                    write("\r\n");
                    break;
                }
                8 | 127 if len > 0 => {
                    len -= 1;
                    write("\x08 \x08");
                }
                32..=126 => {
                    if len < INPUT_LIMIT {
                        line[len] = byte;
                        len += 1;
                        serial_write(byte);
                    } else if !overflowed {
                        overflowed = true;
                        record_error(KernelError::InputTooLong);
                    }
                }
                _ => {}
            }
        }
        if overflowed {
            write("Entrada descartada por exceder o limite.\r\n");
            continue;
        }
        let command = core::str::from_utf8(&line[..len]).unwrap_or("");
        #[cfg(all(target_os = "none", feature = "storage-test"))]
        if matches!(
            command,
            "vfs-teste-gravar" | "vfs-teste-ler" | "vfs-teste-alterar"
        ) {
            if vfs_storage_probe(command).is_err() {
                write("VFS: teste falhou.\r\n");
            }
            continue;
        }
        #[cfg(all(target_os = "none", feature = "storage-test"))]
        if matches!(command, "diskteste-remocao" | "diskteste-timeout") {
            let (result, success) = if command == "diskteste-timeout" {
                (
                    ahci::disposable_timeout_test(),
                    "AHCI: nova leitura após COMRESET OK.\r\n",
                )
            } else {
                (
                    ahci::disposable_removal_test(),
                    "AHCI: remoção detectada; buffer e descritor protegidos.\r\n",
                )
            };
            match result {
                Ok(()) => write(success),
                Err(error) => {
                    record_error(error);
                    write("AHCI: teste falhou: ");
                    write(error.message());
                    write("\r\n");
                }
            }
            continue;
        }
        #[cfg(all(target_os = "none", feature = "storage-test"))]
        if command == "diskteste" {
            write("Teste AHCI em disco QEMU descartável...\r\n");
            match unsafe { ahci::disposable_disk_test() } {
                Ok(()) => {
                    write("AHCI: IDENTIFY, WRITE, READ e erro do controlador verificados.\r\n")
                }
                Err(error) => {
                    record_error(error);
                    write("AHCI: teste falhou: ");
                    write(error.message());
                    write("\r\n");
                }
            }
            continue;
        }
        #[cfg(all(target_os = "none", feature = "storage-test"))]
        if matches!(
            command,
            "log-teste-gravar" | "log-teste-corte" | "log-teste-ler"
        ) {
            use carlio_os::{
                block::BlockRegion,
                persistent_log::{PersistentLog, MAX_RECORD},
                swap::PAGE_SIZE,
            };
            // Only the test build can access this fixed region on the disposable disk.
            const RECORD: &[u8] = b"CarlioOS: persistencia entre boots";
            let result = (|| {
                let mut disk = ahci::block_device().ok()?;
                carlio_os::storage_reserve::StorageReserve::detect(&mut disk).ok()??;
                let region = BlockRegion::new(
                    StorageCutDisk {
                        disk,
                        cut: command == "log-teste-corte",
                        metadata_cut: false,
                    },
                    2048,
                    8,
                )
                .ok()?;
                let mut log = PersistentLog::open(region).ok()?;
                if command == "log-teste-corte" {
                    let _ = log.append(&[0x5a; MAX_RECORD]);
                    return None;
                }
                if command == "log-teste-gravar" {
                    log.append(RECORD).ok().filter(|seq| *seq == 1)?;
                    for sequence in 2..=4 {
                        log.append(RECORD).ok().filter(|&seq| seq == sequence)?;
                    }
                    let mut swap_guard = lock_swap();
                    let swap = swap_guard.as_mut()?;
                    let page = [0x6a; PAGE_SIZE];
                    let mut restored = [0; PAGE_SIZE];
                    let handle = swap.page_out(0, &page).ok()?;
                    swap.page_in(0, &handle, &mut restored).ok()?;
                    swap.release(0, &handle).ok()?;
                    if page != restored {
                        return None;
                    }
                    write("Swap: round-trip cifrado via AHCI OK.\r\n");
                    record_error(KernelError::InputTooLong);
                    flush_persistent_errors();
                    write("Log persistente: gravação e flush OK.\r\n");
                } else {
                    let mut record = [0; MAX_RECORD];
                    let count = log.read(1, &mut record).ok()??;
                    let mut errors = PERSISTENT_ERRORS.lock();
                    let errors = errors.as_mut()?;
                    let mut event = [0; MAX_RECORD];
                    let latest = errors.latest_sequence()?;
                    let found_event = (latest.saturating_sub(7).max(1)..=latest).rev().any(|seq| {
                        errors
                            .read(seq, &mut event)
                            .ok()
                            .flatten()
                            .is_some_and(|len| {
                                &event[..len] == KernelError::InputTooLong.message().as_bytes()
                            })
                    });
                    if &record[..count] != RECORD
                        || log.latest_sequence() != Some(4)
                        || !found_event
                    {
                        return None;
                    }
                    write("Log persistente: leitura após reboot OK.\r\n");
                }
                Some(())
            })();
            if result.is_none() {
                write("Log persistente: teste falhou.\r\n");
            }
            continue;
        }
        #[cfg(all(target_os = "none", feature = "storage-test"))]
        if matches!(command, "fs-meta-corte" | "fs-meta-recuperar") {
            if filesystem_metadata_probe(command == "fs-meta-corte").is_none() {
                write("Filesystem: teste falhou.\r\n");
            }
            continue;
        }
        #[cfg(all(target_os = "none", feature = "storage-test"))]
        if matches!(command, "fs-teste-corte" | "fs-teste-recuperar") {
            if filesystem_cut_probe(command == "fs-teste-corte").is_none() {
                write("Filesystem: teste falhou.\r\n");
            }
            continue;
        }
        #[cfg(all(target_os = "none", feature = "storage-test"))]
        if command == "fs-teste" {
            write("Formatando volume Hermione no disco QEMU descartavel...\r\n");
            match unsafe { ahci::disposable_filesystem_test() } {
                Ok(()) => write(
                    "Hermione: volume persistente, espelhamento, journal/commit, remontagem e scrub passaram.\r\n",
                ),
                Err(error) => {
                    record_error(error);
                    write("Hermione: teste falhou: ");
                    write(error.message());
                    write("\r\n");
                }
            }
            continue;
        }
        #[cfg(feature = "crash-test")]
        match command {
            "panic-teste" => panic!("panic intencional para teste do capturador"),
            "fault-teste" => unsafe { asm!("ud2", options(nomem, nostack)) },
            "hang-teste" => loop {
                core::hint::spin_loop()
            },
            "doublefault-teste" => unsafe {
                asm!("mov rsp, 0", "ud2", options(nomem, nostack, noreturn))
            },
            _ => {}
        }
        match parse_command(command) {
            Command::Help => write(
                "Comandos: ajuda, sobre, usuario, permissoes, erros, logs, limpar, parar.\r\n",
            ),
            Command::About => {
                write("carlIO OS | Criado por João G.R.Santos\r\n");
                write("Sistema operacional experimental x86_64 escrito em Rust.\r\n");
            }
            Command::User => write("usuário: núcleo do sistema (nível de privilégio 0)\r\n"),
            Command::Permissions => write(
                "Política demonstrativa: convidado pode consultar o estado; alteração do sistema é negada.\r\nAviso: os processos ainda não estão isolados; o núcleo inteiro executa no nível 0.\r\n",
            ),
            Command::Errors => report_errors(),
            #[cfg(target_os = "none")]
            Command::Logs => report_errors_markdown(),
            #[cfg(not(target_os = "none"))]
            Command::Logs => {}
            Command::Clear => write("\x1b[2J\x1b[H"),
            Command::Halt => {
                write("Processador parado.\r\n");
                loop {
                    unsafe {
                        asm!("cli; hlt", options(nomem, nostack));
                    }
                }
            }
            Command::Unknown => write("Comando desconhecido. Digite 'ajuda'.\r\n"),
        }
    }
}

#[cfg(target_os = "none")]
fn startup_screen() {
    write("\x1b[2J\x1b[H");
    write("+----------------------------------------------------+\r\n");
    write("| carlIO OS | Criado por João G.R.Santos             |\r\n");
    write("| Sistema operacional experimental x86_64            |\r\n");
    write("+----------------------------------------------------+\r\n");
    write("Núcleo Rust · BOOT LOG\r\n");
    // Row 7/8 stay fixed for progress; only boot messages scroll in rows 10-24.
    write("\x1b[10;1H\x1b[10;24r");
}

/// Atualiza uma linha reservada do terminal ANSI; os eventos continuam sendo
/// impressos abaixo como log serial, inclusive no perfil headless do servidor.
#[cfg(target_os = "none")]
fn boot_progress(percent: u8, stage: &str) {
    const BAR_WIDTH: usize = 78; // brackets plus 78 cells fill an 80-column row.
    let percent = percent.min(100);
    let filled = usize::from(percent) * BAR_WIDTH / 100;
    write("\x1b[s\x1b[7;1H\x1b[2K[");
    for index in 0..BAR_WIDTH {
        if index == 0 {
            write(if filled == 0 {
                "\x1b[90m"
            } else {
                "\x1b[32;1m"
            });
        } else if index == filled {
            write("\x1b[90m");
        }
        write(if index < filled { "#" } else { "-" });
    }
    write("\x1b[0m]\x1b[8;1H\x1b[2KBOOT ");
    if percent < 10 {
        write(" ");
    }
    write_u32(percent as u32);
    write("%  ");
    write(stage);
    write("\x1b[u");
    if percent == 100 {
        write("\x1b[r");
    }
}

#[cfg(target_os = "none")]
fn serial_init() {
    unsafe {
        out(COM1 + 1, 0x00);
        out(COM1 + 3, 0x80);
        out(COM1, 0x03);
        out(COM1 + 1, 0x00);
        out(COM1 + 3, 0x03);
        out(COM1 + 2, 0xc7);
        out(COM1 + 4, 0x0b);
    }
}

#[cfg(target_os = "none")]
fn serial_write(byte: u8) -> bool {
    for iteration in 0..SERIAL_POLL_LIMIT {
        if iteration & 0x0fff == 0 {
            #[cfg(target_os = "none")]
            idt::heartbeat();
        }
        if unsafe { input(COM1 + 5) } & 0x20 != 0 {
            unsafe {
                out(COM1, byte);
            }
            return true;
        }
        core::hint::spin_loop();
    }
    record_error(KernelError::SerialWriteTimeout);
    false
}

#[cfg(target_os = "none")]
fn serial_read() -> Result<u8, KernelError> {
    // APs poll independently so an idle reader cannot monopolize the UART.
    // Ownership covers only the final status recheck and RBR consume, keeping
    // two CPUs from racing on the single receive register.
    for iteration in 0..SERIAL_READ_POLL_LIMIT {
        // SAFETY: COM1 was initialized during boot; this bounded poll only
        // reads the UART line-status register.
        if unsafe { input(COM1 + 5) } & 1 != 0 {
            let Some(_claim) = SERIAL_RX_OWNER.try_claim() else {
                core::hint::spin_loop();
                continue;
            };
            // SAFETY: SERIAL_RX_OWNER grants one CPU exclusive access to the
            // status recheck and receive-buffer register consumption.
            let byte = if unsafe { input(COM1 + 5) } & 1 != 0 {
                Some(unsafe { input(COM1) })
            } else {
                None
            };
            if let Some(byte) = byte {
                return Ok(byte);
            }
        }
        if iteration & 0x0fff == 0 {
            idt::heartbeat();
        }
        core::hint::spin_loop();
    }
    record_error(KernelError::SerialReadTimeout);
    Err(KernelError::SerialReadTimeout)
}

#[cfg(target_os = "none")]
fn write(text: &str) {
    for byte in text.bytes() {
        if !serial_write(byte) {
            break;
        }
    }
}

#[cfg(target_os = "none")]
fn video_boot_probe() {
    use carlio_os::video::{Framebuffer, PixelFormat};

    const MAX_FRAMEBUFFER_BYTES: usize = 128 * 1024 * 1024;
    let response = unsafe { core::ptr::addr_of!(FRAMEBUFFER_REQUEST.response).read_volatile() };
    if response.is_null() {
        write("Vídeo: framebuffer indisponível; kernel continua em modo serial.\r\n");
        return;
    }
    let response = unsafe { &*response };
    if response.framebuffer_count == 0
        || response.framebuffer_count > 8
        || response.framebuffers.is_null()
    {
        write("Vídeo: resposta de framebuffer inválida; modo serial mantido.\r\n");
        return;
    }
    let framebuffer_pointer = unsafe { response.framebuffers.read() };
    if framebuffer_pointer.is_null() {
        write("Vídeo: framebuffer nulo; modo serial mantido.\r\n");
        return;
    }
    let info = unsafe { &*framebuffer_pointer };
    let width = match usize::try_from(info.width) {
        Ok(value) if value <= 8192 => value,
        _ => {
            write("Vídeo: largura de framebuffer inválida.\r\n");
            return;
        }
    };
    let height = match usize::try_from(info.height) {
        Ok(value) if value <= 8192 => value,
        _ => {
            write("Vídeo: altura de framebuffer inválida.\r\n");
            return;
        }
    };
    let pitch = match usize::try_from(info.pitch) {
        Ok(value) => value,
        _ => {
            write("Vídeo: pitch do framebuffer inválido.\r\n");
            return;
        }
    };
    let length = match pitch.checked_mul(height) {
        Some(value) if value <= MAX_FRAMEBUFFER_BYTES => value,
        _ => {
            write("Vídeo: tamanho do framebuffer fora do limite.\r\n");
            return;
        }
    };
    if info.address.is_null()
        || (info.address as usize).checked_add(length).is_none()
        || info.memory_model != 1
    {
        write("Vídeo: endereço ou modelo de pixel não suportado.\r\n");
        return;
    }
    let format = PixelFormat {
        bits_per_pixel: info.bits_per_pixel as u8,
        red_size: info.red_mask_size,
        red_shift: info.red_mask_shift,
        green_size: info.green_mask_size,
        green_shift: info.green_mask_shift,
        blue_size: info.blue_mask_size,
        blue_shift: info.blue_mask_shift,
    };
    let bytes = unsafe { core::slice::from_raw_parts_mut(info.address, length) };
    let mut framebuffer = match Framebuffer::new(bytes, width, height, pitch, format) {
        Ok(fb) => fb,
        Err(_) => {
            write("Vídeo: formato de framebuffer recusado; modo serial mantido.\r\n");
            return;
        }
    };
    let stripe_width = width.min(320) / 4;
    framebuffer.fill_rect(0, 0, width.min(320), height.min(32), [20, 28, 45]);
    framebuffer.fill_rect(8, 8, stripe_width, 12, [230, 75, 75]);
    framebuffer.fill_rect(8 + stripe_width, 8, stripe_width, 12, [80, 200, 120]);
    framebuffer.fill_rect(8 + stripe_width * 2, 8, stripe_width, 12, [80, 140, 240]);
    FRAMEBUFFER_ACTIVE.store(true, Ordering::Release);
    write("Framebuffer Limine: RGB ativo; renderização de pixels validada.\r\n");
}

#[cfg(target_os = "none")]
extern "C" fn timer_process_switch(
    cpu: usize,
    frame: *mut idt::UserInterruptFrame,
) -> *mut idt::UserInterruptFrame {
    if frame.is_null() {
        return frame;
    }
    let previous = PROCESS_SCHEDULER.current(cpu);
    let previous_is_ap_probe =
        is_secondary_cpu(cpu) && previous.is_some_and(|process| is_ap_cpl3_probe_id(process.id));
    if previous_is_ap_probe && frame_user_mode(frame) {
        let bit = 1usize << cpu;
        let active = AP_CPL3_RUNNING_MASK.fetch_and(!bit, Ordering::AcqRel);
        let other_aps = active & secondary_cpu_mask() & !bit;
        if active & bit != 0 && other_aps != 0 {
            AP_CPL3_CPU_MASK.fetch_or(bit | other_aps, Ordering::AcqRel);
            AP_CPL3_OVERLAP_PASSED.store(true, Ordering::Release);
        }
    }
    if AP_CPL3_STOP.load(Ordering::Acquire) && previous_is_ap_probe && frame_user_mode(frame) {
        return frame;
    }
    if PROCESS_PROBE_STARTED.load(Ordering::Acquire) && previous.is_some() && frame_user_mode(frame)
    {
        PROCESS_PROBE_SWITCHES.fetch_add(1, Ordering::Relaxed);
    }
    if let Some(process) = previous {
        user_fpu::save(process);
    }
    let next = if PROCESS_PROBE_STARTED.load(Ordering::Acquire) {
        PROCESS_SCHEDULER.preempt_frame_local(cpu, frame as u64)
    } else {
        PROCESS_SCHEDULER.preempt_frame(cpu, frame as u64)
    };
    let Ok(Some(process)) = next else {
        if previous_is_ap_probe && frame_user_mode(frame) {
            mark_ap_cpl3_running(cpu);
        }
        return frame;
    };
    if PROCESS_PROBE_STARTED.load(Ordering::Acquire)
        && previous.is_some_and(|old| old.id != process.id)
    {
        if PROCESS_PROBE_SWITCHES.load(Ordering::Relaxed) == 1 {
            write("Timer: primeira preempção CPL3 observada.\r\n");
        }
        if previous.is_some_and(|old| old.page_table != process.page_table) {
            PROCESS_PROBE_CR3_SWITCHES.fetch_add(1, Ordering::Relaxed);
        }
        idt::heartbeat();
    }
    if process.interrupt_frame == 0 || process.kernel_stack_top == 0 {
        return frame;
    }
    if !activate_scheduled_process(cpu, process) {
        loop {
            unsafe { asm!("cli; hlt", options(nomem, nostack)) };
        }
    }
    if is_secondary_cpu(cpu) && is_ap_cpl3_probe_id(process.id) {
        mark_ap_cpl3_running(cpu);
    }
    process.interrupt_frame as *mut idt::UserInterruptFrame
}

#[cfg(target_os = "none")]
fn is_ap_cpl3_probe_id(id: u64) -> bool {
    id != 0
        && (id == PROCESS_PROBE_IDS[1].load(Ordering::Acquire)
            || id == PROCESS_PROBE_IDS[2].load(Ordering::Acquire))
}

#[cfg(target_os = "none")]
fn secondary_cpu_mask() -> usize {
    let mut mask = 0usize;
    for cpu in 0..SMP_DETECTED_COUNT
        .load(Ordering::Acquire)
        .min(MAX_BOOT_CPUS)
    {
        if BOOT_CPU_LOCALS[cpu].online.load(Ordering::Acquire) != 0 && is_secondary_cpu(cpu) {
            mask |= 1usize << cpu;
        }
    }
    mask
}

#[cfg(target_os = "none")]
fn mark_ap_cpl3_running(cpu: usize) {
    if cpu < MAX_BOOT_CPUS {
        let bit = 1usize << cpu;
        AP_CPL3_IDLE_MASK.fetch_and(!bit, Ordering::AcqRel);
        let active = AP_CPL3_RUNNING_MASK.fetch_or(bit, Ordering::AcqRel);
        let concurrent = active & secondary_cpu_mask() & !bit;
        if concurrent != 0 {
            AP_CPL3_CPU_MASK.fetch_or(bit | concurrent, Ordering::AcqRel);
            AP_CPL3_OVERLAP_PASSED.store(true, Ordering::Release);
        }
    }
}

#[cfg(target_os = "none")]
pub(crate) fn stop_ap_process_probe(cpu: usize) -> bool {
    if !is_secondary_cpu(cpu) || !AP_CPL3_STOP.load(Ordering::Acquire) {
        return false;
    }
    if PROCESS_SCHEDULER.current(cpu).is_none() {
        return true;
    }
    PROCESS_SCHEDULER.exit(cpu, 0).is_ok()
}

#[cfg(target_os = "none")]
fn frame_user_mode(frame: *mut idt::UserInterruptFrame) -> bool {
    // The interrupt frame is supplied by the x86 entry stub for this CPU.
    unsafe { !frame.is_null() && (*frame).cs & 3 == 3 }
}

#[cfg(target_os = "none")]
pub(crate) fn begin_process_switch_probe() -> Option<*mut idt::UserInterruptFrame> {
    const CODE_VA: u64 = 0x0000_4000_0000_3000;
    const STACK_VA: u64 = CODE_VA + 4096;
    if PROCESS_PROBE_IDS
        .iter()
        .any(|process_id| process_id.load(Ordering::Acquire) != 0)
        || PROCESS_SCHEDULER
            .owned_resources()
            .iter()
            .any(|storage| lock_process_vm_slot(storage).is_some())
    {
        write("Runtime: probe anterior ainda mantém recursos; nova execução bloqueada.\r\n");
        return None;
    }
    if PROCESS_PROBE_STARTED.swap(true, Ordering::AcqRel) {
        return None;
    }
    write("Runtime de processos: preparando tarefas CPL3.\r\n");
    PROCESS_PROBE_SWITCHES.store(0, Ordering::Relaxed);
    PROCESS_PROBE_CR3_SWITCHES.store(0, Ordering::Relaxed);
    PROCESS_PROBE_FAULTS.store(0, Ordering::Relaxed);
    LINUX_GETPID_PROBE_PASSED.store(false, Ordering::Relaxed);
    LINUX_VFS_DENIAL_PROBE_PASSED.store(false, Ordering::Relaxed);
    LINUX_VFS_DENIAL_RESULT.store(0, Ordering::Relaxed);
    LINUX_CREDENTIAL_PROBE_MASK.store(0, Ordering::Relaxed);
    LINUX_MEMORY_PROBE_MASK.store(0, Ordering::Relaxed);
    LINUX_MUNMAP_RESULT.store(0, Ordering::Relaxed);
    AP_CPL3_CPU_MASK.store(0, Ordering::Relaxed);
    AP_CPL3_RUNNING_MASK.store(0, Ordering::Relaxed);
    AP_CPL3_IDLE_MASK.store(0, Ordering::Relaxed);
    AP_CPL3_OVERLAP_PASSED.store(false, Ordering::Relaxed);
    AP_CPL3_STOP.store(false, Ordering::Relaxed);
    for count in &AP_VM_STRESS_MUNMAPS {
        count.store(0, Ordering::Relaxed);
    }
    // O probe entra aqui sob um CR3 temporário. Use o CR3 que o BSP
    // salvou antes de entrar em CPL3 para criar e recolher as tarefas.
    let root = PROCESS_KERNEL_CR3.load(Ordering::Acquire);
    // CPL3 exercises write, identity getters, getgroups, getpid and exit.
    // The write buffer shares the read-only executable page, so the kernel
    // must validate/read it without requiring user write permission.
    let (task, task_len) = linux_syscall_probe_task();
    let (stress_task, stress_task_len) = process_vm_stress_task();
    let image_a = probe_elf_image(&task[..task_len]);
    let image_b = probe_elf_image(&stress_task[..stress_task_len]);
    if !install_process_probe_images(&image_a, &image_b) {
        write("Runtime: não foi possível instalar as imagens no VFS.");
        PROCESS_PROBE_STARTED.store(false, Ordering::Release);
        return None;
    }
    let Some((space_a, entry_a, user_stack_a)) = make_process_probe_space(
        root,
        STACK_VA,
        "/programas/probe",
        &["probe", "--spin"],
        &["PATH=/programas", "CARLIO_PROBE=1"],
    ) else {
        write("Runtime: falha ao criar espaço virtual A.\r\n");
        PROCESS_PROBE_STARTED.store(false, Ordering::Release);
        return None;
    };
    let Some((space_b, entry_b, user_stack_b)) = make_process_probe_space(
        root,
        STACK_VA,
        "/programas/fault",
        &["fault-probe"],
        &["CARLIO_PROBE=1"],
    ) else {
        write("Runtime: falha ao criar espaço virtual B.\r\n");
        let _ = release_or_retain_unregistered_space(space_a);
        PROCESS_PROBE_STARTED.store(false, Ordering::Release);
        return None;
    };
    let Some((space_c, entry_c, user_stack_c)) = make_process_probe_space(
        root,
        STACK_VA,
        "/programas/fault",
        &["ap-probe-2"],
        &["CARLIO_PROBE=1"],
    ) else {
        write("Runtime: falha ao criar espaço virtual C.\r\n");
        let _ = release_or_retain_unregistered_space(space_a);
        let _ = release_or_retain_unregistered_space(space_b);
        PROCESS_PROBE_STARTED.store(false, Ordering::Release);
        return None;
    };
    let root_a = space_a.root();
    let root_b = space_b.root();
    let root_c = space_c.root();
    let mut vm_a = RuntimeProcessVm::new(
        space_a,
        carlio_os::vma::USER_HEAP_BASE,
        carlio_os::vma::USER_HEAP_BASE + 0x1000_0000,
    );
    let mut vm_b = RuntimeProcessVm::new(
        space_b,
        carlio_os::vma::USER_HEAP_BASE,
        carlio_os::vma::USER_HEAP_BASE + 0x1000_0000,
    );
    let mut vm_c = RuntimeProcessVm::new(
        space_c,
        carlio_os::vma::USER_HEAP_BASE,
        carlio_os::vma::USER_HEAP_BASE + 0x1000_0000,
    );
    if vm_a.reserve_existing(CODE_VA, 2 * 4096).is_err()
        || vm_b.reserve_existing(CODE_VA, 2 * 4096).is_err()
        || vm_c.reserve_existing(CODE_VA, 2 * 4096).is_err()
    {
        let _ = release_or_retain_unregistered_vm(vm_a);
        let _ = release_or_retain_unregistered_vm(vm_b);
        let _ = release_or_retain_unregistered_vm(vm_c);
        PROCESS_PROBE_STARTED.store(false, Ordering::Release);
        return None;
    }
    let slot_a = match store_process_vm(vm_a, None) {
        Ok(slot) => slot,
        Err(space) => {
            let _ = release_or_retain_unregistered_vm(space);
            let _ = release_or_retain_unregistered_vm(vm_b);
            let _ = release_or_retain_unregistered_vm(vm_c);
            PROCESS_PROBE_STARTED.store(false, Ordering::Release);
            return None;
        }
    };
    let slot_b = match store_process_vm(vm_b, None) {
        Ok(slot) => slot,
        Err(space) => {
            let _ = release_or_retain_unregistered_vm(space);
            let _ = destroy_process_probe_space_by_root(root_a);
            let _ = release_or_retain_unregistered_vm(vm_c);
            PROCESS_PROBE_STARTED.store(false, Ordering::Release);
            return None;
        }
    };
    let slot_c = match store_process_vm(vm_c, None) {
        Ok(slot) => slot,
        Err(space) => {
            let _ = release_or_retain_unregistered_vm(space);
            let _ = destroy_process_probe_space_by_root(root_a);
            let _ = destroy_process_probe_space_by_root(root_b);
            PROCESS_PROBE_STARTED.store(false, Ordering::Release);
            return None;
        }
    };

    let context_a = CpuContext {
        rip: entry_a,
        rsp: user_stack_a,
        rflags: 0x202,
        ..CpuContext::default()
    };
    let user_probe_security = carlio_os::process::ProcessSecurityContext::new(
        Credentials {
            uid: 1000,
            gid: 1000,
        },
        &[],
        0o022,
    );
    let Some(user_probe_security) = user_probe_security else {
        let _ = abort_unstarted_process_probe([0; 3], [root_a, root_b, root_c]);
        PROCESS_PROBE_STARTED.store(false, Ordering::Release);
        return None;
    };
    let first = PROCESS_SCHEDULER
        .create_with_stack_pool_and_security(
            &PROCESS_KERNEL_STACKS,
            root_a,
            context_a,
            user_probe_security,
        )
        .ok();
    let Some(first) = first else {
        let _ = abort_unstarted_process_probe([0; 3], [root_a, root_b, root_c]);
        PROCESS_PROBE_STARTED.store(false, Ordering::Release);
        return None;
    };
    let context_b = CpuContext {
        rip: entry_b,
        rsp: user_stack_b,
        rflags: 0x202,
        ..CpuContext::default()
    };
    let second =
        match PROCESS_SCHEDULER.create_with_stack_pool(&PROCESS_KERNEL_STACKS, root_b, context_b) {
            Ok(id) => id,
            Err(_) => {
                let _ = abort_unstarted_process_probe([first, 0, 0], [root_a, root_b, root_c]);
                PROCESS_PROBE_STARTED.store(false, Ordering::Release);
                return None;
            }
        };
    let context_c = CpuContext {
        rip: entry_c,
        rsp: user_stack_c,
        rflags: 0x202,
        ..CpuContext::default()
    };
    let third =
        match PROCESS_SCHEDULER.create_with_stack_pool(&PROCESS_KERNEL_STACKS, root_c, context_c) {
            Ok(id) => id,
            Err(_) => {
                let _ = abort_unstarted_process_probe([first, second, 0], [root_a, root_b, root_c]);
                PROCESS_PROBE_STARTED.store(false, Ordering::Release);
                return None;
            }
        };
    let bundles = [
        (first, root_a, user_stack_a, slot_a),
        (second, root_b, user_stack_b, slot_b),
        (third, root_c, user_stack_c, slot_c),
    ];
    for (id, page_table, user_stack, slot) in bundles {
        if !bind_process_vm(slot, id, page_table)
            || PROCESS_SCHEDULER
                .attach_resource_bundle(
                    id,
                    carlio_os::process::ProcessResourceBundle {
                        slot,
                        page_table,
                        user_stack,
                    },
                )
                .is_err()
        {
            let _ = abort_unstarted_process_probe([first, second, third], [root_a, root_b, root_c]);
            PROCESS_PROBE_STARTED.store(false, Ordering::Release);
            return None;
        }
    }
    PROCESS_PROBE_IDS[0].store(first, Ordering::Release);
    PROCESS_PROBE_IDS[1].store(second, Ordering::Release);
    PROCESS_PROBE_IDS[2].store(third, Ordering::Release);
    let bsp_cpu = current_scheduler_cpu();
    let mut secondary_cpus = [usize::MAX; 2];
    let mut secondary_count = 0usize;
    for cpu in 0..SMP_DETECTED_COUNT.load(Ordering::Acquire) {
        if BOOT_CPU_LOCALS[cpu].online.load(Ordering::Acquire) != 0
            && is_secondary_cpu(cpu)
            && secondary_count < secondary_cpus.len()
        {
            secondary_cpus[secondary_count] = cpu;
            secondary_count += 1;
        }
    }
    let pinned = PROCESS_SCHEDULER.pin_ready_to_cpu(first, bsp_cpu).is_ok()
        && secondary_count == secondary_cpus.len()
        && PROCESS_SCHEDULER
            .pin_ready_to_cpu(second, secondary_cpus[0])
            .is_ok()
        && PROCESS_SCHEDULER
            .pin_ready_to_cpu(third, secondary_cpus[1])
            .is_ok();
    if !pinned {
        write("Runtime: afinidade inicial das tarefas CPL3 não pôde ser aplicada.\r\n");
        let _ = abort_unstarted_process_probe([first, second, third], [root_a, root_b, root_c]);
        PROCESS_PROBE_STARTED.store(false, Ordering::Release);
        return None;
    }
    for id in [first, second, third] {
        let Some(process) = PROCESS_SCHEDULER.process(id) else {
            let _ = abort_unstarted_process_probe([first, second, third], [root_a, root_b, root_c]);
            PROCESS_PROBE_STARTED.store(false, Ordering::Release);
            return None;
        };
        let Some(frame) =
            (unsafe { idt::initialize_user_frame(process.kernel_stack_top, process.context) })
        else {
            let _ = abort_unstarted_process_probe([first, second, third], [root_a, root_b, root_c]);
            PROCESS_PROBE_STARTED.store(false, Ordering::Release);
            return None;
        };
        if frame as u64 != process.interrupt_frame {
            let _ = abort_unstarted_process_probe([first, second, third], [root_a, root_b, root_c]);
            PROCESS_PROBE_STARTED.store(false, Ordering::Release);
            return None;
        }
    }
    let cpu = current_scheduler_cpu();
    let process = schedule_process_on_cpu(cpu).ok()??;
    if process.page_table != root {
        PROCESS_PROBE_CR3_SWITCHES.fetch_add(1, Ordering::AcqRel);
    }
    if !activate_scheduled_process(current_scheduler_cpu(), process) {
        PROCESS_PROBE_STARTED.store(false, Ordering::Release);
        return None;
    }
    AP_PROCESS_SCHEDULING.store(true, Ordering::Release);
    write("Runtime: tarefas com CR3 independentes despachadas.\r\n");
    Some(process.interrupt_frame as *mut idt::UserInterruptFrame)
}

#[cfg(target_os = "none")]
fn make_process_probe_space(
    kernel_root: u64,
    stack_va: u64,
    path: &str,
    argv: &[&str],
    env: &[&str],
) -> Option<(
    OwnedAddressSpace<BootPageTableAccess, PROCESS_VM_PAGE_CAPACITY>,
    u64,
    u64,
)> {
    let (space, entry, stack, _) = make_process_space(
        kernel_root,
        stack_va,
        1,
        path,
        argv,
        env,
        AccessContext::new(Credentials { uid: 0, gid: 0 }, &[], 0),
    )
    .ok()?;
    Some((space, entry, stack))
}

#[cfg(target_os = "none")]
fn make_process_space(
    kernel_root: u64,
    stack_va: u64,
    stack_pages: usize,
    path: &str,
    argv: &[&str],
    env: &[&str],
    context: AccessContext<'_>,
) -> Result<
    (
        OwnedAddressSpace<BootPageTableAccess, PROCESS_VM_PAGE_CAPACITY>,
        u64,
        u64,
        [(u64, u64); 2],
    ),
    u16,
> {
    use carlio_os::elf::{ElfError, VfsElfError};
    use carlio_os::linux_abi::{E2BIG, ENOEXEC, ENOMEM};
    let stack_length = (stack_pages as u64).checked_mul(4096).ok_or(E2BIG)?;
    let stack_top = stack_va.checked_add(stack_length).ok_or(E2BIG)?;
    let mut target = match ElfAddressSpace::<PROCESS_VM_PAGE_CAPACITY>::new(kernel_root) {
        Ok(space) => space,
        Err(_) => {
            write("Runtime: nova raiz de paginação recusada.\r\n");
            return Err(ENOMEM);
        }
    };
    let image = {
        let mut program = ELF_LOAD_SCRATCH.lock();
        let mut interpreter = ELF_INTERPRETER_SCRATCH.lock();
        let vfs = PROCESS_VFS.lock();
        let mut backend = ROOTFS_BACKEND.lock();
        let loaded = match backend.as_mut() {
            Some(backend) => carlio_os::elf::load_interpreted_executable_from_mounted::<
                64,
                PROCESS_VFS_CAPACITY,
                _,
                _,
            >(
                &vfs,
                backend,
                path,
                context,
                &mut program[..],
                &mut interpreter[..],
                &mut target,
            ),
            None => {
                carlio_os::elf::load_interpreted_executable_from_vfs::<64, PROCESS_VFS_CAPACITY, _>(
                    &vfs,
                    path,
                    context,
                    &mut program[..],
                    &mut interpreter[..],
                    &mut target,
                )
            }
        };
        match loaded {
            Ok(image) => image,
            Err(error) => {
                write("Runtime: ELF do VFS recusado por acesso, formato ou mapeamento.\r\n");
                let _ = release_or_retain_unregistered_space(target.0);
                return Err(match error {
                    VfsElfError::Vfs(error) => vfs_errno(error),
                    VfsElfError::Elf(ElfError::MappingFailed) => ENOMEM,
                    VfsElfError::Elf(_) => ENOEXEC,
                });
            }
        }
    };
    for page in 0..stack_pages {
        let address = stack_va + (page as u64) * 4096;
        if target
            .0
            .map_zeroed(
                address,
                1 | PAGE_FLAG_USER | PAGE_FLAG_WRITABLE | PAGE_FLAG_NO_EXECUTE,
            )
            .is_err()
        {
            write("Runtime: falha ao mapear stack CPL3.\r\n");
            let _ = release_or_retain_unregistered_space(target.0);
            return Err(ENOMEM);
        }
    }
    let auxv = image.auxv();
    let stack = match carlio_os::elf::build_initial_user_stack_with_auxv_sized(
        &mut target,
        stack_top,
        stack_length,
        argv,
        env,
        &auxv,
    ) {
        Ok(stack) => stack,
        Err(carlio_os::elf::ElfError::InvalidArguments) => {
            write("Runtime: argumentos excedem a stack inicial CPL3.\r\n");
            let _ = release_or_retain_unregistered_space(target.0);
            return Err(E2BIG);
        }
        Err(_) => {
            write("Runtime: escrita da stack inicial CPL3 falhou.\r\n");
            let _ = release_or_retain_unregistered_space(target.0);
            return Err(ENOMEM);
        }
    };
    Ok((
        target.0,
        image.entry,
        stack.stack_pointer,
        image.image_ranges,
    ))
}

#[cfg(target_os = "none")]
fn install_process_probe_images(first: &[u8], second: &[u8]) -> bool {
    let root = Credentials { uid: 0, gid: 0 };
    let access = AccessContext::new(root, &[], 0);
    let mut vfs = PROCESS_VFS.lock();
    if vfs
        .check_access_as("/bin", access, carlio_os::vfs::Access::READ)
        .is_err()
        && vfs.create_fhs_layout().is_err()
    {
        return false;
    }
    if vfs
        .check_access_as("/programas", access, carlio_os::vfs::Access::READ)
        .is_err()
        && vfs.create_dir("/", "programas", root, 0o755).is_err()
    {
        return false;
    }
    let _ = vfs.remove_file_as("/programas/probe", access);
    let _ = vfs.remove_file_as("/programas/fault", access);
    let _ = vfs.remove_file_as("/programas/restrito", access);
    if vfs.create_file("/programas", "probe", root, 0o755).is_err()
        || vfs.create_file("/programas", "fault", root, 0o755).is_err()
        || vfs
            .create_file("/programas", "restrito", root, 0o600)
            .is_err()
    {
        return false;
    }
    vfs.write_file_as("/programas/probe", access, first).is_ok()
        && vfs
            .write_file_as("/programas/fault", access, second)
            .is_ok()
}

#[cfg(target_os = "none")]
fn probe_elf_image(task: &[u8]) -> [u8; 1024] {
    use carlio_os::elf::{ELF_HEADER_SIZE, PF_R, PF_X, PROGRAM_HEADER_SIZE, PT_LOAD};
    let mut elf = [0u8; 1024];
    elf[..4].copy_from_slice(b"\x7fELF");
    elf[4] = 2;
    elf[5] = 1;
    elf[6] = 1;
    elf[16..18].copy_from_slice(&2u16.to_le_bytes());
    elf[18..20].copy_from_slice(&62u16.to_le_bytes());
    elf[20..24].copy_from_slice(&1u32.to_le_bytes());
    elf[24..32].copy_from_slice(&0x0000_4000_0000_3000u64.to_le_bytes());
    elf[32..40].copy_from_slice(&(ELF_HEADER_SIZE as u64).to_le_bytes());
    elf[52..54].copy_from_slice(&(ELF_HEADER_SIZE as u16).to_le_bytes());
    elf[54..56].copy_from_slice(&(PROGRAM_HEADER_SIZE as u16).to_le_bytes());
    elf[56..58].copy_from_slice(&1u16.to_le_bytes());
    let ph = ELF_HEADER_SIZE;
    elf[ph..ph + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
    elf[ph + 4..ph + 8].copy_from_slice(&(PF_R | PF_X).to_le_bytes());
    elf[ph + 8..ph + 16].copy_from_slice(&120u64.to_le_bytes());
    elf[ph + 16..ph + 24].copy_from_slice(&0x0000_4000_0000_3000u64.to_le_bytes());
    elf[ph + 32..ph + 40].copy_from_slice(&(task.len() as u64 + 2).to_le_bytes());
    elf[ph + 40..ph + 48].copy_from_slice(&(task.len() as u64 + 2).to_le_bytes());
    elf[ph + 48..ph + 56].copy_from_slice(&1u64.to_le_bytes());
    elf[120..120 + task.len()].copy_from_slice(task);
    elf[120 + task.len()] = 0xcd;
    elf[121 + task.len()] = 0x80;
    elf
}

#[cfg(target_os = "none")]
pub(crate) fn process_runtime_exit(code: u64) -> Option<*mut idt::UserInterruptFrame> {
    let cpu = current_scheduler_cpu();
    let previous = PROCESS_SCHEDULER.current(cpu)?;
    PROCESS_SCHEDULER.exit(cpu, code as i64).ok()?;
    if is_ap_cpl3_probe_id(previous.id) {
        AP_CPL3_RUNNING_MASK.fetch_and(!(1usize << cpu), Ordering::AcqRel);
    }
    let process = schedule_process_on_cpu(cpu).ok()??;
    if !activate_scheduled_process(cpu, process) {
        return None;
    }
    Some(process.interrupt_frame as *mut idt::UserInterruptFrame)
}

#[cfg(target_os = "none")]
pub(crate) fn process_runtime_fault(
    cpu: usize,
    vector: u64,
    error_code: u64,
    cr2: u64,
) -> Option<*mut idt::UserInterruptFrame> {
    let victim = PROCESS_SCHEDULER.current(cpu)?;
    PROCESS_SCHEDULER
        .fault(cpu, vector as u8, error_code, cr2)
        .ok()?;
    if is_ap_cpl3_probe_id(victim.id) {
        AP_CPL3_RUNNING_MASK.fetch_and(!(1usize << cpu), Ordering::AcqRel);
    }
    if PROCESS_PROBE_STARTED.load(Ordering::Acquire) {
        PROCESS_PROBE_FAULTS.fetch_add(1, Ordering::AcqRel);
    }
    write("Runtime: fault CPL3 vetorizado=");
    write_u64(vector);
    if vector == 14 {
        write(" CR2=0x");
        write_hex_u64(cr2);
        write(" erro=");
        write_u64(error_code);
    }
    write("; encerrando somente a tarefa.\r\n");
    let process = schedule_process_on_cpu(cpu).ok()??;
    if !activate_scheduled_process(cpu, process) {
        return None;
    }
    Some(process.interrupt_frame as *mut idt::UserInterruptFrame)
}

#[cfg(target_os = "none")]
fn linux_syscall_probe_task() -> ([u8; 1024], usize) {
    const CODE_VA: u64 = 0x0000_4000_0000_3000;
    const STACK_VA: u64 = CODE_VA + 4096;
    const MESSAGE: &[u8] = b"write ok\n";
    let probe_path: &[u8] = if cfg!(feature = "disk-read-test") {
        b"/probe.bin\0"
    } else {
        b"/programas/probe\0"
    };
    let mut code = [0u8; 1024];
    let mut length = 0usize;
    // Sample the scheduled compatibility entry before the native calls.
    append_probe_bytes(&mut code, &mut length, &[0xb8, 39, 0, 0, 0, 0xcd, 0x80]);
    // A fixture precisa permanecer em CPL3 por pelo menos um tick do PIT
    // mesmo quando o QEMU executa rapidamente as demais instruções.
    append_probe_bytes(&mut code, &mut length, &[0xb9, 0x00, 0x00, 0x00, 0x01]);
    append_probe_bytes(&mut code, &mut length, &[0xe2, 0xfe]); // loop
    append_probe_bytes(&mut code, &mut length, &[0xbf, 1, 0, 0, 0]); // write(1, ...)
    append_probe_bytes(&mut code, &mut length, &[0x48, 0xbe]);
    let message_pointer_offset = length;
    append_probe_bytes(&mut code, &mut length, &[0; 8]);
    append_probe_bytes(
        &mut code,
        &mut length,
        &[
            0xba,
            MESSAGE.len() as u8,
            0,
            0,
            0,
            0xb8,
            1,
            0,
            0,
            0,
            0x0f,
            0x05,
        ],
    );
    for number in [
        carlio_os::linux_abi::SYS_GETUID,
        carlio_os::linux_abi::SYS_GETEUID,
        carlio_os::linux_abi::SYS_GETGID,
        carlio_os::linux_abi::SYS_GETEGID,
    ] {
        append_probe_syscall(&mut code, &mut length, number);
    }
    append_probe_bytes(&mut code, &mut length, &[0x31, 0xff, 0x31, 0xf6]);
    append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_GETGROUPS);
    // openat(AT_FDCWD, probe_path, O_RDONLY), then read one byte
    // into writable user stack memory and close the descriptor.
    append_probe_bytes(&mut code, &mut length, &[0x48, 0xbf]);
    append_probe_bytes(&mut code, &mut length, &((-100i64) as u64).to_le_bytes());
    append_probe_bytes(&mut code, &mut length, &[0x48, 0xbe]);
    let path_pointer_offset = length;
    append_probe_bytes(&mut code, &mut length, &[0; 8]);
    append_probe_bytes(&mut code, &mut length, &[0x31, 0xd2]);
    append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_OPENAT);
    append_probe_bytes(&mut code, &mut length, &[0x48, 0x89, 0xc7, 0x48, 0xbe]);
    append_probe_bytes(&mut code, &mut length, &(STACK_VA + 2048).to_le_bytes());
    append_probe_bytes(&mut code, &mut length, &[0xba, 1, 0, 0, 0]);
    append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_READ);
    append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_CLOSE);
    append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_GETPID);
    append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_GETTID);
    append_probe_bytes(&mut code, &mut length, &[0xbf, 1, 0, 0, 0, 0x48, 0xbe]);
    append_probe_bytes(&mut code, &mut length, &(STACK_VA + 2064).to_le_bytes());
    append_probe_syscall(
        &mut code,
        &mut length,
        carlio_os::linux_abi::SYS_CLOCK_GETTIME,
    );
    append_probe_bytes(&mut code, &mut length, &[0x31, 0xff, 0x48, 0xbe]);
    append_probe_bytes(
        &mut code,
        &mut length,
        &0x0000_7000_0000_0000u64.to_le_bytes(),
    );
    append_probe_bytes(&mut code, &mut length, &[0xba, 1, 0, 0, 0]);
    append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_READ);
    // read(0) positivo: o harness QEMU envia um byte após o aviso do kernel.
    append_probe_bytes(&mut code, &mut length, &[0xbf, 0, 0, 0, 0, 0x48, 0xbe]);
    append_probe_bytes(&mut code, &mut length, &(STACK_VA + 2050).to_le_bytes());
    append_probe_bytes(&mut code, &mut length, &[0xba, 1, 0, 0, 0]);
    append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_READ);
    // mmap an anonymous private page, write through the returned mapping, and
    // release it again through munmap.
    append_probe_bytes(
        &mut code,
        &mut length,
        &[
            0x31, 0xff, // rdi = hint NULL
            0xbe, 0x00, 0x10, 0, 0, // rsi = 4096
            0xba, 3, 0, 0, 0, // rdx = PROT_READ | PROT_WRITE
            0x41, 0xba, 0x22, 0, 0, 0, // r10 = MAP_PRIVATE | MAP_ANONYMOUS
            0x49, 0xc7, 0xc0, 0xff, 0xff, 0xff, 0xff, // r8 = -1
            0x45, 0x31, 0xc9, // r9 = 0
        ],
    );
    append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_MMAP);
    append_probe_bytes(&mut code, &mut length, &[0xc6, 0x00, 0x51]); // *rax = 'Q'
    append_probe_bytes(&mut code, &mut length, &[0x48, 0x89, 0xc3]); // rbx = mapping
    if cfg!(feature = "storage-test") {
        append_probe_bytes(&mut code, &mut length, &[0x48, 0x89, 0xdf]); // rdi = mapping
        append_probe_bytes(&mut code, &mut length, &[0xbe, 0, 0x10, 0, 0]); // len
        append_probe_bytes(&mut code, &mut length, &[0xba, 21, 0, 0, 0]); // MADV_PAGEOUT
        append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_MADVISE);
        // cmp byte [rbx], 'Q'; je past ud2. The load must fault and retry.
        append_probe_bytes(
            &mut code,
            &mut length,
            &[0x80, 0x3b, 0x51, 0x74, 0x02, 0x0f, 0x0b],
        );
    }
    append_probe_bytes(&mut code, &mut length, &[0x48, 0x89, 0xdf]); // rdi = mapping
    append_probe_bytes(&mut code, &mut length, &[0xbe, 0x00, 0x10, 0, 0]);
    append_probe_bytes(&mut code, &mut length, &[0xba, 1, 0, 0, 0]); // PROT_READ
    append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_MPROTECT);
    append_probe_bytes(&mut code, &mut length, &[0x48, 0x89, 0xdf]); // rdi = mapping
    append_probe_bytes(&mut code, &mut length, &[0xbe, 0x00, 0x10, 0, 0]);
    append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_MUNMAP);
    // brk cresce uma página, grava no limite novo e retorna ao break inicial.
    append_probe_bytes(&mut code, &mut length, &[0x48, 0xbf]);
    append_probe_bytes(
        &mut code,
        &mut length,
        &(carlio_os::vma::USER_HEAP_BASE + 4096).to_le_bytes(),
    );
    append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_BRK);
    append_probe_bytes(
        &mut code,
        &mut length,
        &[0x48, 0x83, 0xe8, 0x01, 0xc6, 0x00, 0x42],
    );
    append_probe_bytes(&mut code, &mut length, &[0x48, 0xbf]);
    append_probe_bytes(
        &mut code,
        &mut length,
        &carlio_os::vma::USER_HEAP_BASE.to_le_bytes(),
    );
    append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_BRK);
    // An unprivileged process must receive EACCES for a root-owned 0600 file.
    append_probe_bytes(&mut code, &mut length, &[0x48, 0xbf]);
    append_probe_bytes(&mut code, &mut length, &((-100i64) as u64).to_le_bytes());
    append_probe_bytes(&mut code, &mut length, &[0x48, 0xbe]);
    let denied_path_pointer_offset = length;
    append_probe_bytes(&mut code, &mut length, &[0; 8]);
    append_probe_bytes(&mut code, &mut length, &[0x31, 0xd2]);
    append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_OPENAT);
    let disk_path_pointer = if cfg!(feature = "disk-exec-test") {
        append_probe_bytes(&mut code, &mut length, &[0x66, 0x0f, 0x76, 0xff]); // pcmpeqd xmm7, xmm7
        append_probe_bytes(&mut code, &mut length, &[0x48, 0xbf]); // rdi = path
        let pointer = length;
        append_probe_bytes(&mut code, &mut length, &[0; 8]);
        append_probe_bytes(&mut code, &mut length, &[0x31, 0xf6, 0x31, 0xd2]); // argv/envp
        append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_EXECVE);
        Some(pointer)
    } else {
        None
    };
    append_probe_bytes(&mut code, &mut length, &[0x31, 0xff]);
    append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_EXIT);
    let message_offset = length;
    append_probe_bytes(&mut code, &mut length, MESSAGE);
    let path_offset = length;
    append_probe_bytes(&mut code, &mut length, probe_path);
    let denied_path_offset = length;
    append_probe_bytes(&mut code, &mut length, b"/programas/restrito\0");
    code[message_pointer_offset..message_pointer_offset + 8]
        .copy_from_slice(&(CODE_VA + message_offset as u64).to_le_bytes());
    code[path_pointer_offset..path_pointer_offset + 8]
        .copy_from_slice(&(CODE_VA + path_offset as u64).to_le_bytes());
    code[denied_path_pointer_offset..denied_path_pointer_offset + 8]
        .copy_from_slice(&(CODE_VA + denied_path_offset as u64).to_le_bytes());
    if let Some(pointer) = disk_path_pointer {
        let address = CODE_VA + length as u64;
        append_probe_bytes(&mut code, &mut length, b"/carlio-disk-exec\0");
        code[pointer..pointer + 8].copy_from_slice(&address.to_le_bytes());
    }
    (code, length)
}

#[cfg(target_os = "none")]
fn process_vm_stress_task() -> ([u8; 1024], usize) {
    let mut code = [0u8; 1024];
    let mut length = 0usize;
    append_probe_bytes(&mut code, &mut length, &[0x41, 0xbc]); // mov r12d, iterations
    let iterations = match u32::try_from(AP_VM_STRESS_TARGET) {
        Ok(iterations) => iterations,
        Err(_) => 0,
    };
    append_probe_bytes(&mut code, &mut length, &iterations.to_le_bytes());
    append_probe_bytes(
        &mut code,
        &mut length,
        &[
            0x31, 0xff, // xor edi, edi (hint)
            0xbe, 0x00, 0x10, 0x00, 0x00, // mov esi, 4096
            0xba, 0x03, 0x00, 0x00, 0x00, // mov edx, PROT_READ|PROT_WRITE
            0x41, 0xba, 0x22, 0x00, 0x00, 0x00, // mov r10d, MAP_PRIVATE|MAP_ANONYMOUS
            0x49, 0xc7, 0xc0, 0xff, 0xff, 0xff, 0xff, // mov r8, -1
            0x45, 0x31, 0xc9, // xor r9d, r9d (offset)
            0xb8, 0x09, 0x00, 0x00, 0x00, // mmap
            0x0f, 0x05, 0x48, 0x89, 0xc7, // mov rdi, rax
            0xbe, 0x00, 0x10, 0x00, 0x00, // mov esi, 4096
            0xb8, 0x0b, 0x00, 0x00, 0x00, // munmap
            0x0f, 0x05, 0x41, 0xff, 0xcc, // dec r12d
            0x75, 0xc9, // jnz to mmap loop (-55 bytes)
        ],
    );
    if cfg!(feature = "smp-stress-test") {
        // Read unmapped VA zero after the workload to exercise the real #PF path.
        append_probe_bytes(&mut code, &mut length, &[0x31, 0xc0, 0x8a, 0x00]);
    } else {
        append_probe_bytes(&mut code, &mut length, &[0xeb, 0xfe]);
    }
    (code, length)
}

#[cfg(target_os = "none")]
fn append_probe_syscall(code: &mut [u8; 1024], length: &mut usize, number: u64) {
    append_probe_bytes(code, length, &[0xb8]); // mov eax, imm32
    append_probe_bytes(code, length, &(number as u32).to_le_bytes());
    append_probe_bytes(code, length, &[0x0f, 0x05]);
}

#[cfg(target_os = "none")]
fn append_probe_bytes(code: &mut [u8; 1024], length: &mut usize, bytes: &[u8]) {
    let end = *length + bytes.len();
    code[*length..end].copy_from_slice(bytes);
    *length = end;
}

#[cfg(target_os = "none")]
fn cpu_index_for_lapic(lapic_id: u32) -> Option<usize> {
    BOOT_CPU_LOCALS.iter().position(|cpu| {
        cpu.online.load(Ordering::Acquire) != 0 && cpu.lapic_id.load(Ordering::Relaxed) == lapic_id
    })
}

#[cfg(target_os = "none")]
fn current_scheduler_cpu() -> usize {
    lapic::local_id().and_then(cpu_index_for_lapic).unwrap_or(0)
}

#[cfg(target_os = "none")]
fn activate_scheduled_process(cpu: usize, process: carlio_os::process::Process) -> bool {
    if !unsafe {
        idt::activate_cpu_process_address_space(cpu, process.page_table, process.kernel_stack_top)
    } {
        return false;
    }
    user_fpu::restore(process);
    true
}

#[cfg(target_os = "none")]
fn schedule_process_on_cpu(
    cpu: usize,
) -> Result<Option<carlio_os::process::Process>, carlio_os::process::ProcessError> {
    if is_secondary_cpu(cpu) && PROCESS_PROBE_STARTED.load(Ordering::Acquire) {
        PROCESS_SCHEDULER.schedule_local(cpu)
    } else {
        PROCESS_SCHEDULER.schedule(cpu)
    }
}

#[cfg(target_os = "none")]
pub(crate) fn scheduler_has_current_process(cpu: usize) -> bool {
    PROCESS_SCHEDULER.current(cpu).is_some()
}

#[cfg(target_os = "none")]
pub(crate) fn dispatch_linux_syscall(
    number: u64,
    arguments: [u64; 6],
    cpu: usize,
    via_int80: bool,
) -> Option<carlio_os::linux_abi::SyscallOutcome> {
    let process = PROCESS_SCHEDULER.current(cpu)?;
    let mut io = KernelLinuxSyscallIo { process, cpu };
    let program_break = io.with_process_vm(|vm| Ok(vm.program_break())).unwrap_or(0);
    let outcome = carlio_os::linux_abi::dispatch(
        number,
        arguments,
        carlio_os::linux_abi::ProcessContext {
            process_id: process.thread_group_id,
            thread_id: process.id,
            uid: process.security.credentials.uid,
            gid: process.security.credentials.gid,
            supplementary_groups: process.security.supplementary_groups,
            group_count: process.security.group_count,
            program_break,
        },
        &mut io,
    );
    if number == carlio_os::linux_abi::SYS_OPENAT && process.security.credentials.uid != 0 {
        if let carlio_os::linux_abi::SyscallOutcome::Return(value) = outcome {
            LINUX_VFS_DENIAL_RESULT.store(value, Ordering::Release);
            if value == (-(carlio_os::linux_abi::EACCES as i64)) as u64 {
                LINUX_VFS_DENIAL_PROBE_PASSED.store(true, Ordering::Release);
            }
        }
    }
    let mut sample_stack = !cfg!(feature = "smp-stress-test") || !is_ap_cpl3_probe_id(process.id);
    if number == carlio_os::linux_abi::SYS_MUNMAP {
        if let carlio_os::linux_abi::SyscallOutcome::Return(value) = outcome {
            LINUX_MUNMAP_RESULT.store(value, Ordering::Release);
            if value == 0 {
                for (counter, process_index) in AP_VM_STRESS_MUNMAPS.iter().zip([1usize, 2]) {
                    if process.id == PROCESS_PROBE_IDS[process_index].load(Ordering::Acquire) {
                        if !cfg!(feature = "smp-stress-test") || heap_stress_cycle(process.id) {
                            let completed = counter.fetch_add(1, Ordering::AcqRel) + 1;
                            sample_stack |= completed == AP_VM_STRESS_TARGET;
                        }
                    }
                }
            }
        }
    }
    let identity_bit = match number {
        carlio_os::linux_abi::SYS_GETUID
            if outcome
                == carlio_os::linux_abi::SyscallOutcome::Return(
                    process.security.credentials.uid as u64,
                ) =>
        {
            1
        }
        carlio_os::linux_abi::SYS_GETEUID
            if outcome
                == carlio_os::linux_abi::SyscallOutcome::Return(
                    process.security.credentials.uid as u64,
                ) =>
        {
            2
        }
        carlio_os::linux_abi::SYS_GETGID
            if outcome
                == carlio_os::linux_abi::SyscallOutcome::Return(
                    process.security.credentials.gid as u64,
                ) =>
        {
            4
        }
        carlio_os::linux_abi::SYS_GETEGID
            if outcome
                == carlio_os::linux_abi::SyscallOutcome::Return(
                    process.security.credentials.gid as u64,
                ) =>
        {
            8
        }
        carlio_os::linux_abi::SYS_GETGROUPS
            if arguments[0] == 0
                && outcome
                    == carlio_os::linux_abi::SyscallOutcome::Return(
                        process.security.group_count as u64,
                    ) =>
        {
            16
        }
        carlio_os::linux_abi::SYS_WRITE
            if arguments[0] == 1
                && outcome == carlio_os::linux_abi::SyscallOutcome::Return(arguments[2]) =>
        {
            32
        }
        carlio_os::linux_abi::SYS_READ
            if arguments[0] == 0
                && arguments[2] == 1
                && outcome
                    == carlio_os::linux_abi::SyscallOutcome::Return(
                        (-(carlio_os::linux_abi::EFAULT as i64)) as u64,
                    ) =>
        {
            64
        }
        carlio_os::linux_abi::SYS_READ
            if arguments[0] >= carlio_os::open_files::FILE_FD_BASE as u64
                && matches!(outcome, carlio_os::linux_abi::SyscallOutcome::Return(amount) if amount > 0) =>
        {
            128
        }
        carlio_os::linux_abi::SYS_READ
            if arguments[0] == 0
                && arguments[1] == 0x0000_4000_0000_4802
                && arguments[2] == 1
                && matches!(outcome, carlio_os::linux_abi::SyscallOutcome::Return(1)) =>
        {
            256
        }
        _ => 0,
    };
    if identity_bit != 0 {
        LINUX_CREDENTIAL_PROBE_MASK.fetch_or(identity_bit, Ordering::AcqRel);
    }
    if number == carlio_os::linux_abi::SYS_GETPID
        && outcome == carlio_os::linux_abi::SyscallOutcome::Return(process.thread_group_id)
    {
        LINUX_GETPID_PROBE_PASSED.store(true, Ordering::Release);
    }
    match (number, outcome) {
        (carlio_os::linux_abi::SYS_GETTID, carlio_os::linux_abi::SyscallOutcome::Return(tid))
            if tid == process.id =>
        {
            LINUX_CREDENTIAL_PROBE_MASK.fetch_or(0x200, Ordering::AcqRel);
        }
        (
            carlio_os::linux_abi::SYS_CLOCK_GETTIME,
            carlio_os::linux_abi::SyscallOutcome::Return(0),
        ) if arguments[0] == carlio_os::linux_abi::CLOCK_MONOTONIC
            && arguments[1] == 0x0000_4000_0000_4810 =>
        {
            LINUX_CREDENTIAL_PROBE_MASK.fetch_or(0x400, Ordering::AcqRel);
        }
        _ => {}
    }
    match (number, outcome) {
        (carlio_os::linux_abi::SYS_MMAP, carlio_os::linux_abi::SyscallOutcome::Return(address))
            if io
                .with_process_vm(|vm| {
                    vm.mapping(address)
                        .map(|mapping| {
                            mapping.flags & PAGE_FLAG_USER != 0
                                && mapping.flags & PAGE_FLAG_WRITABLE != 0
                        })
                        .map_err(|_| carlio_os::linux_abi::EFAULT)
                })
                .unwrap_or(false) =>
        {
            LINUX_MEMORY_PROBE_MASK.fetch_or(0x1, Ordering::AcqRel);
        }
        (carlio_os::linux_abi::SYS_MPROTECT, carlio_os::linux_abi::SyscallOutcome::Return(0))
            if io
                .with_process_vm(|vm| {
                    vm.mapping(arguments[0])
                        .map(|mapping| {
                            mapping.flags & PAGE_FLAG_USER != 0
                                && mapping.flags & PAGE_FLAG_WRITABLE == 0
                                && mapping.flags & PAGE_FLAG_NO_EXECUTE != 0
                        })
                        .map_err(|_| carlio_os::linux_abi::EFAULT)
                })
                .unwrap_or(false) =>
        {
            LINUX_MEMORY_PROBE_MASK.fetch_or(0x10, Ordering::AcqRel);
        }
        (carlio_os::linux_abi::SYS_MUNMAP, carlio_os::linux_abi::SyscallOutcome::Return(0))
            if io
                .with_process_vm(|vm| Ok(vm.mapping(arguments[0]) == Err(FrameError::NotMapped)))
                .unwrap_or(false) =>
        {
            LINUX_MEMORY_PROBE_MASK.fetch_or(0x2, Ordering::AcqRel);
        }
        (carlio_os::linux_abi::SYS_BRK, carlio_os::linux_abi::SyscallOutcome::Return(address))
            if arguments[0] == carlio_os::vma::USER_HEAP_BASE + 4096
                && address == arguments[0]
                && io
                    .with_process_vm(|vm| {
                        vm.mapping(carlio_os::vma::USER_HEAP_BASE)
                            .map(|mapping| mapping.flags & PAGE_FLAG_WRITABLE != 0)
                            .map_err(|_| carlio_os::linux_abi::EFAULT)
                    })
                    .unwrap_or(false) =>
        {
            LINUX_MEMORY_PROBE_MASK.fetch_or(0x4, Ordering::AcqRel);
        }
        (carlio_os::linux_abi::SYS_BRK, carlio_os::linux_abi::SyscallOutcome::Return(address))
            if arguments[0] == carlio_os::vma::USER_HEAP_BASE
                && address == arguments[0]
                && io
                    .with_process_vm(|vm| {
                        Ok(
                            vm.mapping(carlio_os::vma::USER_HEAP_BASE)
                                == Err(FrameError::NotMapped),
                        )
                    })
                    .unwrap_or(false) =>
        {
            LINUX_MEMORY_PROBE_MASK.fetch_or(0x8, Ordering::AcqRel);
        }
        _ => {}
    }
    // The cumulative mark on the stress tasks is read after the final cycle.
    if sample_stack {
        measure_process_syscall_stack(process.kernel_stack_top, via_int80);
    }
    Some(outcome)
}

#[cfg(target_os = "none")]
pub(crate) fn commit_exec_transition(transition: carlio_os::linux_abi::ExecTransition) {
    user_fpu::reset();
    // The replacement address space keeps the kernel mappings and active
    // syscall stack. CR3 reload is the commit point before releasing old pages.
    unsafe {
        asm!(
            "mov cr3, {}",
            in(reg) transition.page_table,
            options(nostack, preserves_flags)
        );
    }
    match process_vm_slot(transition.old_resource_slot) {
        Some(storage)
            if destroy_process_vm(
                storage,
                transition.old_page_table,
                Some(transition.process_id),
            ) => {}
        _ => write("execve: CR3 novo ativo; VM antigo preservado para retry.\r\n"),
    }
}

#[cfg(target_os = "none")]
struct KernelLinuxSyscallIo {
    process: carlio_os::process::Process,
    cpu: usize,
}

#[cfg(target_os = "none")]
impl KernelLinuxSyscallIo {
    fn restore_user_range(&mut self, address: u64, length: u64) -> Result<(), u16> {
        if length == 0 || BOOT_SWAP.try_lock().is_some_and(|swap| swap.is_none()) {
            return Ok(());
        }
        let end = address
            .checked_add(length)
            .ok_or(carlio_os::linux_abi::EFAULT)?;
        let owner = self.process.thread_group_id;
        self.with_process_vm(|vm| {
            if let Some(page) = vm.swapped_address() {
                if page < end && page + 4096 > address {
                    restore_vm_page(vm, owner, page)?;
                }
            }
            Ok(())
        })
    }

    fn with_process_vm<R>(
        &mut self,
        operation: impl FnOnce(&mut RuntimeProcessVm) -> Result<R, u16>,
    ) -> Result<R, u16> {
        let owner = if self.process.is_thread() {
            PROCESS_SCHEDULER
                .process(self.process.thread_group_id)
                .ok_or(carlio_os::linux_abi::EFAULT)?
        } else {
            self.process
        };
        let bundle = owner
            .resources
            .filter(|bundle| bundle.page_table == self.process.page_table)
            .ok_or(carlio_os::linux_abi::ENOMEM)?;
        let slot = process_vm_slot(bundle.slot).ok_or(carlio_os::linux_abi::ENOMEM)?;
        let mut guard = lock_process_vm_slot(slot);
        let entry = guard.as_mut().ok_or(carlio_os::linux_abi::ENOMEM)?;
        if entry.owner != Some(owner.id) || entry.value.root() != bundle.page_table {
            return Err(carlio_os::linux_abi::EFAULT);
        }
        operation(&mut entry.value)
    }
}

#[cfg(target_os = "none")]
fn lock_swap() -> carlio_os::sync::IrqSpinLockGuard<'static, Option<KernelSwap>> {
    loop {
        if let Some(guard) = BOOT_SWAP.try_lock() {
            return guard;
        }
        lapic::wait_for_shootdown_progress();
    }
}

#[cfg(target_os = "none")]
fn swap_frame_ptr(physical: u64) -> Result<*mut carlio_os::swap::Page, FrameError> {
    let hhdm = unsafe { core::ptr::addr_of!(HHDM_OFFSET).read_volatile() };
    hhdm.checked_add(physical)
        .map(|address| address as *mut carlio_os::swap::Page)
        .ok_or(FrameError::InvalidPhysicalAddress)
}

#[cfg(target_os = "none")]
fn restore_vm_page(vm: &mut RuntimeProcessVm, owner: u64, address: u64) -> Result<bool, u16> {
    if vm.swapped_address() != Some(address & !0xfff) {
        return Ok(false);
    }
    let mut store = lock_swap();
    let swap = store.as_mut().ok_or(carlio_os::linux_abi::EIO)?;
    vm.page_in(address, owner, swap, |physical, page| {
        // The new frame is exclusively held by this VM and is unpublished.
        unsafe { swap_frame_ptr(physical)?.write(*page) };
        Ok(())
    })
    .map_err(process_vm_errno)
}

#[cfg(target_os = "none")]
pub(crate) fn process_swap_fault(cpu: usize, error: u64, address: u64) -> bool {
    // A present or malformed PTE is a protection fault. Only CPL3
    // not-present faults reach this path.
    if error & (1 | 8) != 0 {
        return false;
    }
    let Some(process) = PROCESS_SCHEDULER.current(cpu) else {
        return false;
    };
    let mut context = KernelLinuxSyscallIo { process, cpu };
    let restored = context
        .with_process_vm(|vm| restore_vm_page(vm, process.thread_group_id, address))
        == Ok(true);
    #[cfg(feature = "storage-test")]
    if restored {
        write("Swap VM: page fault CPL3 restaurado.\r\n");
    }
    restored
}

#[cfg(target_os = "none")]
fn process_vm_slot(slot: u8) -> Option<&'static IrqSpinLock<Option<ProcessVmSlot>>> {
    PROCESS_SCHEDULER.owned_resources().get(usize::from(slot))
}

#[cfg(target_os = "none")]
fn lock_process_vm_slot(
    slot: &IrqSpinLock<Option<ProcessVmSlot>>,
) -> carlio_os::sync::IrqSpinLockGuard<'_, Option<ProcessVmSlot>> {
    loop {
        if let Some(guard) = slot.try_lock() {
            return guard;
        }
        // The owner may be waiting for a TLB acknowledgement from this CPU.
        lapic::wait_for_shootdown_progress();
    }
}

#[cfg(target_os = "none")]
fn bind_process_vm(slot: u8, id: u64, root: u64) -> bool {
    let Some(storage) = process_vm_slot(slot) else {
        return false;
    };
    let mut guard = lock_process_vm_slot(storage);
    let Some(entry) = guard.as_mut() else {
        return false;
    };
    if entry.owner.is_some() || entry.value.root() != root {
        return false;
    }
    entry.owner = Some(id);
    true
}

#[cfg(target_os = "none")]
fn store_process_vm(space: RuntimeProcessVm, owner: Option<u64>) -> Result<u8, RuntimeProcessVm> {
    // A VM operation on another CPU can hold its slot while waiting for our
    // TLB shootdown acknowledgement. Skip contended slots with IRQs restored.
    for (slot, storage) in (0u8..).zip(PROCESS_SCHEDULER.owned_resources()) {
        if let Some(mut guard) = storage.try_lock() {
            if guard.is_none() {
                *guard = Some(ProcessVmSlot {
                    owner,
                    value: space,
                });
                return Ok(slot);
            }
        }
    }
    Err(space)
}

#[cfg(target_os = "none")]
fn release_or_retain_unregistered_vm(space: RuntimeProcessVm) -> bool {
    match space.try_destroy() {
        Ok(_access) => true,
        Err((space, _error)) => {
            write("Runtime: teardown pré-dispatch falhou; VM mantido no pool para retry.\r\n");
            match store_process_vm(space, None) {
                Ok(_slot) => false,
                Err(space) => {
                    write("Runtime: pool sem slot para preservar VM não liberado.\r\n");
                    core::mem::forget(space);
                    false
                }
            }
        }
    }
}

#[cfg(target_os = "none")]
fn release_or_retain_unregistered_space(
    space: OwnedAddressSpace<BootPageTableAccess, PROCESS_VM_PAGE_CAPACITY>,
) -> bool {
    match space.try_destroy() {
        Ok(_access) => true,
        Err((space, _error)) => {
            write("Runtime: teardown pré-dispatch falhou; espaço mantido para retry.\r\n");
            let vm = RuntimeProcessVm::new(
                space,
                carlio_os::vma::USER_HEAP_BASE,
                carlio_os::vma::USER_HEAP_BASE + 0x1000_0000,
            );
            match store_process_vm(vm, None) {
                Ok(_slot) => false,
                Err(vm) => {
                    write("Runtime: pool sem slot para preservar espaço não liberado.\r\n");
                    core::mem::forget(vm);
                    false
                }
            }
        }
    }
}

#[cfg(target_os = "none")]
fn process_probe_vm_memory(process_id: u64) -> Option<carlio_os::vma::ProcessMemoryUsage> {
    let process = PROCESS_SCHEDULER.process(process_id)?;
    let bundle = process.resources?;
    let storage = process_vm_slot(bundle.slot)?;
    let guard = lock_process_vm_slot(storage);
    let entry = guard.as_ref()?;
    if entry.owner != Some(process.id) || entry.value.root() != process.page_table {
        return None;
    }
    entry.value.memory_usage().ok()
}

#[cfg(target_os = "none")]
impl carlio_os::linux_abi::LinuxSyscallIo for KernelLinuxSyscallIo {
    fn clock_gettime(&mut self, clock_id: u64) -> Result<carlio_os::linux_abi::Timespec, u16> {
        if clock_id != carlio_os::linux_abi::CLOCK_MONOTONIC {
            return Err(carlio_os::linux_abi::EINVAL);
        }
        let ticks = idt::uptime_ticks();
        let seconds = i64::try_from(ticks / idt::TICK_RATE_HZ)
            .map_err(|_| carlio_os::linux_abi::EOVERFLOW)?;
        let subsecond_ticks = ticks % idt::TICK_RATE_HZ;
        let nanoseconds = i64::try_from(subsecond_ticks)
            .ok()
            .and_then(|value| value.checked_mul(1_000_000_000))
            .and_then(|value| value.checked_div(i64::try_from(idt::TICK_RATE_HZ).ok()?))
            .ok_or(carlio_os::linux_abi::EOVERFLOW)?;
        Ok(carlio_os::linux_abi::Timespec {
            seconds,
            nanoseconds,
        })
    }

    fn clock_getres(&mut self, clock_id: u64) -> Result<carlio_os::linux_abi::Timespec, u16> {
        if clock_id != carlio_os::linux_abi::CLOCK_MONOTONIC {
            return Err(carlio_os::linux_abi::EINVAL);
        }
        Ok(carlio_os::linux_abi::Timespec {
            seconds: 0,
            nanoseconds: i64::try_from(1_000_000_000 / idt::TICK_RATE_HZ)
                .map_err(|_| carlio_os::linux_abi::EOVERFLOW)?,
        })
    }

    fn mmap(
        &mut self,
        address: u64,
        length: u64,
        protection: u64,
        flags: u64,
        descriptor: u64,
        offset: u64,
    ) -> Result<u64, u16> {
        let owner = self.process.thread_group_id;
        self.with_process_vm(|vm| {
            if let Some(page) = vm.swapped_address() {
                restore_vm_page(vm, owner, page)?;
            }
            vm.mmap_linux(address, length, protection, flags, descriptor, offset)
                .map_err(process_vm_errno)
        })
    }

    fn munmap(&mut self, address: u64, length: u64) -> Result<(), u16> {
        self.restore_user_range(address, length)?;
        self.with_process_vm(|vm| vm.munmap(address, length).map_err(process_vm_errno))
    }

    fn mprotect(&mut self, address: u64, length: u64, protection: u64) -> Result<(), u16> {
        self.restore_user_range(address, length)?;
        self.with_process_vm(|vm| {
            vm.mprotect(address, length, protection)
                .map_err(process_vm_errno)
        })
    }

    fn madvise(&mut self, address: u64, length: u64, advice: u64) -> Result<(), u16> {
        use carlio_os::linux_abi::{EINVAL, EOPNOTSUPP, MADV_PAGEOUT};

        if advice != MADV_PAGEOUT || self.process.is_thread() {
            return Err(EOPNOTSUPP);
        }
        if length != 4096 || !address.is_multiple_of(4096) {
            return Err(EINVAL);
        }
        let owner = self.process.id;
        self.with_process_vm(|vm| {
            let mut store = lock_swap();
            let swap = store.as_mut().ok_or(EOPNOTSUPP)?;
            vm.evict_anonymous(address, owner, swap, |physical| {
                // The running process is the only current reader/writer of its frame.
                Ok(unsafe { swap_frame_ptr(physical)?.read() })
            })
            .map_err(process_vm_errno)
        })
    }

    fn brk(&mut self, current: u64, requested: u64) -> u64 {
        let owner = self.process.thread_group_id;
        self.with_process_vm(|vm| {
            if let Some(page) = vm.swapped_address() {
                restore_vm_page(vm, owner, page)?;
            }
            vm.brk(requested).map_err(process_vm_errno)
        })
        .unwrap_or(current)
    }

    fn copy_from_user(&mut self, address: u64, destination: &mut [u8]) -> Result<(), u16> {
        self.restore_user_range(address, destination.len() as u64)?;
        let hhdm = unsafe { core::ptr::addr_of!(HHDM_OFFSET).read_volatile() };
        let lookup = PageTableManager::new(BootPageTableAccess { hhdm }, self.process.page_table)
            .map_err(|_| carlio_os::linux_abi::EFAULT)?;
        unsafe { carlio_os::user_memory::copy_from_user(&lookup, address, destination) }
            .map_err(|_| carlio_os::linux_abi::EFAULT)
    }

    fn copy_to_user(&mut self, address: u64, source: &[u8]) -> Result<(), u16> {
        self.restore_user_range(address, source.len() as u64)?;
        let hhdm = unsafe { core::ptr::addr_of!(HHDM_OFFSET).read_volatile() };
        let lookup = PageTableManager::new(BootPageTableAccess { hhdm }, self.process.page_table)
            .map_err(|_| carlio_os::linux_abi::EFAULT)?;
        unsafe { carlio_os::user_memory::copy_to_user(&lookup, address, source) }
            .map_err(|_| carlio_os::linux_abi::EFAULT)
    }

    fn validate_user_write(&mut self, address: u64, length: usize) -> Result<(), u16> {
        self.restore_user_range(address, length as u64)?;
        let hhdm = unsafe { core::ptr::addr_of!(HHDM_OFFSET).read_volatile() };
        let lookup = PageTableManager::new(BootPageTableAccess { hhdm }, self.process.page_table)
            .map_err(|_| carlio_os::linux_abi::EFAULT)?;
        carlio_os::user_memory::validate(&lookup, address, length, true)
            .map_err(|_| carlio_os::linux_abi::EFAULT)
    }

    fn execve(
        &mut self,
        path: &str,
        arguments: &[&str],
        environment: &[&str],
    ) -> Result<carlio_os::linux_abi::ExecTransition, u16> {
        if self.process.is_thread() {
            return Err(carlio_os::linux_abi::EBUSY);
        }
        let kernel_root = PROCESS_KERNEL_CR3.load(Ordering::Acquire);
        if kernel_root == 0 {
            return Err(carlio_os::linux_abi::EIO);
        }
        let stack_top = carlio_os::elf::USER_TOP;
        let stack_pages = 4usize;
        let stack_base = stack_top
            .checked_sub((stack_pages as u64) * 4096)
            .ok_or(carlio_os::linux_abi::E2BIG)?;
        let context = self.process.security.access_context();
        let (space, instruction_pointer, stack_pointer, image_ranges) = make_process_space(
            kernel_root,
            stack_base,
            stack_pages,
            path,
            arguments,
            environment,
            context,
        )?;
        let mut new_vm = RuntimeProcessVm::new(
            space,
            carlio_os::vma::USER_HEAP_BASE,
            carlio_os::vma::USER_HEAP_BASE + 0x1000_0000,
        );
        for (start, length) in image_ranges {
            if length != 0 && new_vm.reserve_existing(start, length).is_err() {
                let _ = release_or_retain_unregistered_vm(new_vm);
                return Err(carlio_os::linux_abi::ENOMEM);
            }
        }
        let stack_length = (stack_pages as u64) * 4096;
        if new_vm
            .reserve_existing(stack_top - stack_length, stack_length)
            .is_err()
        {
            let _ = release_or_retain_unregistered_vm(new_vm);
            return Err(carlio_os::linux_abi::ENOMEM);
        }
        let new_page_table = new_vm.root();
        let new_slot = match store_process_vm(new_vm, Some(self.process.id)) {
            Ok(slot) => slot,
            Err(vm) => {
                let _ = release_or_retain_unregistered_vm(vm);
                return Err(carlio_os::linux_abi::ENOMEM);
            }
        };
        let new_bundle = carlio_os::process::ProcessResourceBundle {
            slot: new_slot,
            page_table: new_page_table,
            user_stack: stack_pointer,
        };
        let old_bundle = match PROCESS_SCHEDULER.replace_current_image(
            self.cpu,
            self.process.id,
            new_bundle,
            instruction_pointer,
            stack_pointer,
        ) {
            Ok(bundle) => bundle,
            Err(error) => {
                let _ = destroy_process_vm(
                    process_vm_slot(new_slot).ok_or(carlio_os::linux_abi::ENOMEM)?,
                    new_page_table,
                    Some(self.process.id),
                );
                return Err(match error {
                    carlio_os::process::ProcessError::Busy => carlio_os::linux_abi::EBUSY,
                    carlio_os::process::ProcessError::Full => carlio_os::linux_abi::EAGAIN,
                    _ => carlio_os::linux_abi::EFAULT,
                });
            }
        };
        Ok(carlio_os::linux_abi::ExecTransition {
            process_id: self.process.id,
            old_page_table: old_bundle.page_table,
            old_resource_slot: old_bundle.slot,
            page_table: new_page_table,
            instruction_pointer,
            stack_pointer,
        })
    }

    fn read(&mut self, descriptor: u64, destination: &mut [u8]) -> Result<usize, u16> {
        if descriptor == 0 {
            write("ABI Linux: probe CPL3 aguardando byte de entrada serial.\r\n");
            for (index, output) in destination.iter_mut().enumerate() {
                let byte = match serial_read() {
                    Ok(byte) => byte,
                    Err(_) if index > 0 => return Ok(index),
                    Err(_) => return Err(carlio_os::linux_abi::EIO),
                };
                *output = if byte == b'\r' { b'\n' } else { byte };
                if *output == b'\n' {
                    return Ok(index + 1);
                }
            }
            return Ok(destination.len());
        }
        let descriptor = u32::try_from(descriptor).map_err(|_| carlio_os::linux_abi::EBADF)?;
        let file = OPEN_FILES
            .lock()
            .get(self.process.id, descriptor)
            .map_err(|_| carlio_os::linux_abi::EBADF)?;
        if !file.readable {
            return Err(carlio_os::linux_abi::EBADF);
        }
        let path = file.path_str().ok_or(carlio_os::linux_abi::EIO)?;
        let mut scratch = [0u8; 256];
        let chunk = destination.len().min(scratch.len());
        let context = self.process.security.access_context();
        let offset = usize::try_from(file.offset).map_err(|_| carlio_os::linux_abi::EINVAL)?;
        let inline =
            PROCESS_VFS
                .lock()
                .read_file_range_as(path, context, offset, &mut scratch[..chunk]);
        let amount = match inline {
            Ok(amount) => amount,
            Err(carlio_os::vfs::VfsError::NotFound) => {
                read_rootfs_file(path, context, file.offset, &mut scratch[..chunk])
                    .map_err(vfs_errno)?
            }
            Err(error) => return Err(vfs_errno(error)),
        };
        destination[..amount].copy_from_slice(&scratch[..amount]);
        OPEN_FILES
            .lock()
            .set_offset(self.process.id, descriptor, file.offset + amount as u64)
            .map_err(|_| carlio_os::linux_abi::EBADF)?;
        Ok(usize::try_from(amount).map_err(|_| carlio_os::linux_abi::EOVERFLOW)?)
    }

    fn open(&mut self, path: &str, flags: u64, mode: u64) -> Result<u64, u16> {
        let _transaction = VfsMutationGuard::acquire()?;
        let create_mode = if flags & 0x40 != 0 {
            u16::try_from(mode).map_err(|_| carlio_os::linux_abi::EINVAL)?
        } else {
            0
        };
        let access = match flags & 3 {
            0 => carlio_os::vfs::Access::READ,
            1 => carlio_os::vfs::Access::WRITE,
            2 => carlio_os::vfs::Access::READ_WRITE,
            _ => return Err(carlio_os::linux_abi::EINVAL),
        };
        let readable = flags & 3 != 1;
        let writable = flags & 3 != 0;
        let append = flags & 0x400 != 0;
        let descriptor = OPEN_FILES
            .lock()
            .reserve(self.process.id, path, readable, writable, append)
            .map_err(|_| carlio_os::linux_abi::ENFILE)?;
        let operation = (|| {
            let context = self.process.security.access_context();
            if flags & 0x200 != 0 && !writable {
                return Err(carlio_os::linux_abi::EACCES);
            }
            let metadata = PROCESS_VFS.lock().metadata(path, context);
            let overlay_metadata = match metadata {
                Ok(metadata) => Some(metadata),
                Err(carlio_os::vfs::VfsError::NotFound) => None,
                Err(error) => return Err(vfs_errno(error)),
            };
            let base_metadata = if overlay_metadata.is_none() {
                match rootfs_metadata(path, context) {
                    Ok(metadata) => Some(metadata),
                    Err(
                        carlio_os::vfs::VfsError::NotFound | carlio_os::vfs::VfsError::NotMounted,
                    ) => None,
                    Err(error) => return Err(vfs_errno(error)),
                }
            } else {
                None
            };
            let mut vfs = PROCESS_VFS.lock();
            let mut changed = false;
            if flags & 0x80 != 0
                && flags & 0x40 != 0
                && (overlay_metadata.is_some() || base_metadata.is_some())
            {
                return Err(carlio_os::linux_abi::EEXIST);
            }
            if let Some(metadata) = overlay_metadata {
                if metadata.kind == carlio_os::vfs::NodeKind::Directory {
                    return Err(carlio_os::linux_abi::EISDIR);
                }
                vfs.check_access_as(path, context, access)
                    .map_err(vfs_errno)?;
            } else if let Some(metadata) = base_metadata {
                if metadata.kind == carlio_os::vfs::NodeKind::Directory {
                    return Err(carlio_os::linux_abi::EISDIR);
                }
                let requested =
                    (if readable { 0b100 } else { 0 }) | (if writable { 0b010 } else { 0 });
                if !mount_permission(metadata, context, requested) {
                    return Err(carlio_os::linux_abi::EACCES);
                }
                if writable {
                    if metadata.size > carlio_os::vfs::VFS_INLINE_FILE_MAX as u64 {
                        return Err(carlio_os::linux_abi::EFBIG);
                    }
                    let (parent, name) =
                        ensure_overlay_parent(&mut vfs, path, context).map_err(vfs_errno)?;
                    vfs.create_file_as(parent, name, context, metadata.mode)
                        .map_err(vfs_errno)?;
                    let mut scratch = ROOTFS_COPY_SCRATCH.lock();
                    let mut backend = ROOTFS_BACKEND.lock();
                    let backend = backend.as_mut().ok_or(carlio_os::linux_abi::EIO)?;
                    let mount = vfs.mount_for(path).ok_or(carlio_os::linux_abi::EIO)?;
                    let relative = if mount.path == "/" {
                        path
                    } else {
                        &path[mount.path.len()..]
                    };
                    let length = carlio_os::vfs::ReadOnlyMountBackend::read_file(
                        backend,
                        relative,
                        &mut scratch[..metadata.size as usize],
                    )
                    .map_err(vfs_errno)?;
                    vfs.write_file_as(path, context, &scratch[..length])
                        .map_err(vfs_errno)?;
                    changed = true;
                }
            } else if flags & 0x40 != 0 {
                let (parent, name) =
                    ensure_overlay_parent(&mut vfs, path, context).map_err(vfs_errno)?;
                vfs.create_file_as(parent, name, context, create_mode)
                    .map_err(vfs_errno)?;
                changed = true;
            } else {
                return Err(carlio_os::linux_abi::ENOENT);
            }
            if flags & 0x200 != 0 {
                vfs.truncate_file_as(path, context).map_err(vfs_errno)?;
                changed = true;
            }
            let initial_offset = if append {
                u64::try_from(
                    vfs.file_len_as(path, context, carlio_os::vfs::Access::WRITE)
                        .map_err(vfs_errno)?,
                )
                .map_err(|_| carlio_os::linux_abi::EOVERFLOW)?
            } else {
                0
            };
            drop(vfs);
            if changed {
                commit_vfs_mutation(&_transaction)?;
            }
            let mut files = OPEN_FILES.lock();
            if initial_offset != 0 {
                files
                    .set_offset(self.process.id, descriptor, initial_offset)
                    .map_err(|_| carlio_os::linux_abi::EBADF)?;
            }
            files
                .commit(self.process.id, descriptor)
                .map_err(|_| carlio_os::linux_abi::EBADF)
        })();
        if operation.is_err() {
            let _ = OPEN_FILES.lock().close(self.process.id, descriptor);
        }
        operation.map(|()| descriptor as u64)
    }

    fn write(&mut self, descriptor: u64, bytes: &[u8]) -> Result<usize, u16> {
        if descriptor == 1 || descriptor == 2 {
            for byte in bytes {
                if !serial_write(*byte) {
                    return Err(carlio_os::linux_abi::EIO);
                }
            }
            return Ok(bytes.len());
        }
        let descriptor = u32::try_from(descriptor).map_err(|_| carlio_os::linux_abi::EBADF)?;
        let _transaction = VfsMutationGuard::acquire()?;
        let file = OPEN_FILES
            .lock()
            .get(self.process.id, descriptor)
            .map_err(|_| carlio_os::linux_abi::EBADF)?;
        if !file.writable {
            return Err(carlio_os::linux_abi::EBADF);
        }
        let path = file.path_str().ok_or(carlio_os::linux_abi::EIO)?;
        let context = self.process.security.access_context();
        let mut vfs = PROCESS_VFS.lock();
        let offset = if file.append {
            u64::try_from(
                vfs.file_len_as(path, context, carlio_os::vfs::Access::WRITE)
                    .map_err(vfs_errno)?,
            )
            .map_err(|_| carlio_os::linux_abi::EOVERFLOW)?
        } else {
            file.offset
        };
        let amount = vfs
            .write_file_range_as(
                path,
                context,
                usize::try_from(offset).map_err(|_| carlio_os::linux_abi::EFBIG)?,
                bytes,
            )
            .map_err(vfs_errno)?;
        drop(vfs);
        commit_vfs_mutation(&_transaction)?;
        let amount_u64 = u64::try_from(amount).map_err(|_| carlio_os::linux_abi::EOVERFLOW)?;
        let next_offset = offset
            .checked_add(amount_u64)
            .ok_or(carlio_os::linux_abi::EOVERFLOW)?;
        OPEN_FILES
            .lock()
            .set_offset(self.process.id, descriptor, next_offset)
            .map_err(|_| carlio_os::linux_abi::EBADF)?;
        Ok(amount)
    }

    fn close(&mut self, descriptor: u64) -> Result<(), u16> {
        let descriptor = u32::try_from(descriptor).map_err(|_| carlio_os::linux_abi::EBADF)?;
        if descriptor >= carlio_os::open_files::FILE_FD_BASE {
            return OPEN_FILES
                .lock()
                .close(self.process.id, descriptor)
                .map_err(|_| carlio_os::linux_abi::EBADF);
        }
        NETWORK_SOCKETS
            .lock()
            .close(self.process.id, descriptor)
            .map_err(network_errno)
    }

    fn fsync(&mut self, descriptor: u64) -> Result<(), u16> {
        let descriptor = u32::try_from(descriptor).map_err(|_| carlio_os::linux_abi::EBADF)?;
        OPEN_FILES
            .lock()
            .get(self.process.id, descriptor)
            .map_err(|_| carlio_os::linux_abi::EBADF)?;
        let transaction = VfsMutationGuard::acquire()?;
        commit_vfs_mutation(&transaction)
    }

    fn fstat(&mut self, descriptor: u64) -> Result<carlio_os::linux_abi::LinuxStat, u16> {
        let descriptor = u32::try_from(descriptor).map_err(|_| carlio_os::linux_abi::EBADF)?;
        let file = OPEN_FILES
            .lock()
            .get(self.process.id, descriptor)
            .map_err(|_| carlio_os::linux_abi::EBADF)?;
        let path = file.path_str().ok_or(carlio_os::linux_abi::EIO)?;
        self.stat(path)
    }

    fn stat(&mut self, path: &str) -> Result<carlio_os::linux_abi::LinuxStat, u16> {
        let context = self.process.security.access_context();
        let vfs = PROCESS_VFS.lock();
        let metadata = match vfs.metadata(path, context) {
            Ok(metadata) => carlio_os::vfs::MountMetadata {
                inode: metadata.inode,
                kind: metadata.kind,
                uid: metadata.uid,
                gid: metadata.gid,
                mode: metadata.mode,
                size: metadata.size,
            },
            Err(carlio_os::vfs::VfsError::NotFound) => {
                drop(vfs);
                rootfs_metadata(path, context).map_err(vfs_errno)?
            }
            Err(error) => return Err(vfs_errno(error)),
        };
        let size = i64::try_from(metadata.size).map_err(|_| carlio_os::linux_abi::EOVERFLOW)?;
        let blocks = size
            .checked_add(511)
            .and_then(|rounded| rounded.checked_div(512))
            .ok_or(carlio_os::linux_abi::EOVERFLOW)?;
        let mode = (match metadata.kind {
            carlio_os::vfs::NodeKind::File => 0o100000,
            carlio_os::vfs::NodeKind::Directory => 0o040000,
        }) | u32::from(metadata.mode & 0o7777);
        Ok(carlio_os::linux_abi::LinuxStat {
            inode: metadata.inode,
            mode,
            links: 1,
            uid: metadata.uid,
            gid: metadata.gid,
            size,
            block_size: 4096,
            blocks,
            access_time: carlio_os::linux_abi::Timespec {
                seconds: 0,
                nanoseconds: 0,
            },
            modification_time: carlio_os::linux_abi::Timespec {
                seconds: 0,
                nanoseconds: 0,
            },
            change_time: carlio_os::linux_abi::Timespec {
                seconds: 0,
                nanoseconds: 0,
            },
        })
    }

    fn access(&mut self, path: &str, mode: u64) -> Result<(), u16> {
        let mode = u8::try_from(mode).map_err(|_| carlio_os::linux_abi::EINVAL)?;
        let requested =
            carlio_os::vfs::Access::from_posix_bits(mode).ok_or(carlio_os::linux_abi::EINVAL)?;
        let context = self.process.security.access_context();
        match PROCESS_VFS.lock().check_access_as(path, context, requested) {
            Ok(_) => Ok(()),
            Err(carlio_os::vfs::VfsError::NotFound) => {
                let metadata = rootfs_metadata(path, context).map_err(vfs_errno)?;
                if mount_permission(metadata, context, mode) {
                    Ok(())
                } else {
                    Err(carlio_os::linux_abi::EACCES)
                }
            }
            Err(error) => Err(vfs_errno(error)),
        }
    }

    fn mkdir(&mut self, path: &str, mode: u16) -> Result<(), u16> {
        let transaction = VfsMutationGuard::acquire()?;
        let context = self.process.security.access_context();
        match rootfs_metadata(path, context) {
            Ok(_) => return Err(carlio_os::linux_abi::EEXIST),
            Err(carlio_os::vfs::VfsError::NotFound | carlio_os::vfs::VfsError::NotMounted) => {}
            Err(error) => return Err(vfs_errno(error)),
        }
        let mut vfs = PROCESS_VFS.lock();
        let (parent, name) = ensure_overlay_parent(&mut vfs, path, context).map_err(vfs_errno)?;
        vfs.create_dir_as(parent, name, context, mode)
            .map_err(vfs_errno)?;
        drop(vfs);
        commit_vfs_mutation(&transaction)
    }

    fn umask(&mut self, mask: u16) -> Result<u16, u16> {
        let previous = self.process.security.umask;
        self.process.security.umask = mask & 0o777;
        Ok(previous)
    }

    fn chmod(&mut self, path: &str, mode: u16) -> Result<(), u16> {
        let transaction = VfsMutationGuard::acquire()?;
        PROCESS_VFS
            .lock()
            .set_mode_as(path, self.process.security.access_context(), mode)
            .map_err(vfs_errno)?;
        commit_vfs_mutation(&transaction)
    }

    fn fchmod(&mut self, descriptor: u64, mode: u16) -> Result<(), u16> {
        let descriptor = u32::try_from(descriptor).map_err(|_| carlio_os::linux_abi::EBADF)?;
        let file = OPEN_FILES
            .lock()
            .get(self.process.id, descriptor)
            .map_err(|_| carlio_os::linux_abi::EBADF)?;
        let path = file.path_str().ok_or(carlio_os::linux_abi::EIO)?;
        self.chmod(path, mode)
    }

    fn chown(&mut self, path: &str, uid: Option<u32>, gid: Option<u32>) -> Result<(), u16> {
        if self.process.security.credentials.uid != 0 {
            return Err(carlio_os::linux_abi::EPERM);
        }
        let transaction = VfsMutationGuard::acquire()?;
        let context = self.process.security.access_context();
        let mut vfs = PROCESS_VFS.lock();
        let metadata = vfs.metadata(path, context).map_err(vfs_errno)?;
        vfs.set_owner_as(
            path,
            context,
            uid.unwrap_or(metadata.uid),
            gid.unwrap_or(metadata.gid),
        )
        .map_err(vfs_errno)?;
        drop(vfs);
        commit_vfs_mutation(&transaction)
    }

    fn fchown(&mut self, descriptor: u64, uid: Option<u32>, gid: Option<u32>) -> Result<(), u16> {
        let descriptor = u32::try_from(descriptor).map_err(|_| carlio_os::linux_abi::EBADF)?;
        let file = OPEN_FILES
            .lock()
            .get(self.process.id, descriptor)
            .map_err(|_| carlio_os::linux_abi::EBADF)?;
        let path = file.path_str().ok_or(carlio_os::linux_abi::EIO)?;
        self.chown(path, uid, gid)
    }

    fn unlink(&mut self, path: &str) -> Result<(), u16> {
        let transaction = VfsMutationGuard::acquire()?;
        if OPEN_FILES.lock().is_path_open(path) {
            return Err(carlio_os::linux_abi::EBUSY);
        }
        PROCESS_VFS
            .lock()
            .remove_file_as(path, self.process.security.access_context())
            .map_err(vfs_errno)?;
        commit_vfs_mutation(&transaction)
    }

    fn rmdir(&mut self, path: &str) -> Result<(), u16> {
        let transaction = VfsMutationGuard::acquire()?;
        PROCESS_VFS
            .lock()
            .remove_dir_as(path, self.process.security.access_context())
            .map_err(vfs_errno)?;
        commit_vfs_mutation(&transaction)
    }

    fn seek(&mut self, descriptor: u64, offset: i64, whence: u64) -> Result<u64, u16> {
        let descriptor = u32::try_from(descriptor).map_err(|_| carlio_os::linux_abi::EBADF)?;
        let file = OPEN_FILES
            .lock()
            .get(self.process.id, descriptor)
            .map_err(|_| carlio_os::linux_abi::EBADF)?;
        let base = match whence {
            0 => 0i64,
            1 => i64::try_from(file.offset).map_err(|_| carlio_os::linux_abi::EINVAL)?,
            2 => {
                let access = if file.readable {
                    carlio_os::vfs::Access::READ
                } else {
                    carlio_os::vfs::Access::WRITE
                };
                PROCESS_VFS
                    .lock()
                    .file_len_as(
                        file.path_str().ok_or(carlio_os::linux_abi::EIO)?,
                        self.process.security.access_context(),
                        access,
                    )
                    .map_err(vfs_errno)? as i64
            }
            _ => return Err(carlio_os::linux_abi::EINVAL),
        };
        let next = base
            .checked_add(offset)
            .filter(|value| *value >= 0)
            .ok_or(carlio_os::linux_abi::EINVAL)? as u64;
        OPEN_FILES
            .lock()
            .set_offset(self.process.id, descriptor, next)
            .map_err(|_| carlio_os::linux_abi::EBADF)?;
        Ok(next)
    }

    fn socket(&mut self, domain: u64, kind: u64, protocol: u64) -> Result<u64, u16> {
        NETWORK_SOCKETS
            .lock()
            .create(
                self.process.id,
                self.process.security.credentials.uid,
                domain,
                kind,
                protocol,
            )
            .map(|fd| fd as u64)
            .map_err(network_errno)
    }

    fn bind(&mut self, descriptor: u64, address: [u8; 4], port: u16) -> Result<(), u16> {
        let descriptor = u32::try_from(descriptor).map_err(|_| carlio_os::linux_abi::EBADF)?;
        NETWORK_SOCKETS
            .lock()
            .bind(
                self.process.id,
                self.process.security.credentials.uid,
                descriptor,
                address,
                port,
            )
            .map_err(network_errno)
    }

    fn listen(&mut self, descriptor: u64, backlog: u16) -> Result<(), u16> {
        let descriptor = u32::try_from(descriptor).map_err(|_| carlio_os::linux_abi::EBADF)?;
        NETWORK_SOCKETS
            .lock()
            .listen(self.process.id, descriptor, backlog)
            .map_err(network_errno)
    }
}

#[cfg(target_os = "none")]
fn process_vm_errno(error: carlio_os::vma::ProcessVmError) -> u16 {
    use carlio_os::vma::{ProcessVmError as E, VmaError as V};
    match error {
        E::Vma(V::InvalidRange | V::Overlap) => carlio_os::linux_abi::EINVAL,
        E::Vma(V::Capacity | V::HeapCollision) | E::Memory(FrameError::CapacityExceeded) => {
            carlio_os::linux_abi::ENOMEM
        }
        E::Vma(V::NotMapped) => carlio_os::linux_abi::EINVAL,
        E::UnsupportedMapping => carlio_os::linux_abi::EOPNOTSUPP,
        E::PermissionDenied => carlio_os::linux_abi::EACCES,
        E::Memory(FrameError::InvalidVirtualAddress | FrameError::InvalidPageFlags) => {
            carlio_os::linux_abi::EINVAL
        }
        E::Memory(_) | E::RollbackFailed | E::Swap => carlio_os::linux_abi::EIO,
    }
}

#[cfg(target_os = "none")]
fn network_errno(error: carlio_os::net::NetworkError) -> u16 {
    use carlio_os::net::NetworkError as N;
    match error {
        N::AddressInUse => carlio_os::linux_abi::EADDRINUSE,
        N::PermissionDenied => carlio_os::linux_abi::EACCES,
        N::BadDescriptor => carlio_os::linux_abi::EBADF,
        N::UnsupportedSocket => carlio_os::linux_abi::EAFNOSUPPORT,
        N::InvalidPort | N::InvalidState => carlio_os::linux_abi::EINVAL,
        _ => carlio_os::linux_abi::ENFILE,
    }
}

#[cfg(target_os = "none")]
fn vfs_errno(error: carlio_os::vfs::VfsError) -> u16 {
    use carlio_os::vfs::VfsError as V;
    match error {
        V::NotFound | V::NotMounted => carlio_os::linux_abi::ENOENT,
        V::AlreadyExists => carlio_os::linux_abi::EEXIST,
        V::DirectoryNotEmpty => carlio_os::linux_abi::ENOTEMPTY,
        V::Busy => carlio_os::linux_abi::EBUSY,
        V::PermissionDenied | V::NotOwner | V::MountDenied | V::NotExecutable => {
            carlio_os::linux_abi::EACCES
        }
        V::IsDirectory => carlio_os::linux_abi::EISDIR,
        V::FileTooLarge => carlio_os::linux_abi::EFBIG,
        V::Full | V::MountTableFull => carlio_os::linux_abi::ENFILE,
        V::InvalidPath | V::BufferTooSmall => carlio_os::linux_abi::EINVAL,
        V::NotDirectory => carlio_os::linux_abi::ENOTDIR,
        _ => carlio_os::linux_abi::EIO,
    }
}

#[cfg(target_os = "none")]
pub(crate) fn is_secondary_cpu(cpu: usize) -> bool {
    BOOT_CPU_LOCALS.get(cpu).is_some_and(|local| {
        local.lapic_id.load(Ordering::Acquire) != SMP_BSP_LAPIC_ID.load(Ordering::Acquire)
    })
}

/// Assembly hook sampled after syscall dispatch, before restoring the user frame.
#[cfg(target_os = "none")]
fn measure_process_syscall_stack(top: u64, via_int80: bool) {
    #[cfg(feature = "stack-watermark")]
    // Both entries dispatch on the task stack while the scheduler owns it on this CPU.
    if let Some(bytes) = unsafe { PROCESS_KERNEL_STACKS.high_water_bytes_for_top(top) } {
        let index = usize::from(via_int80);
        SYSCALL_STACK_SAMPLES[index].fetch_add(1, Ordering::Relaxed);
        SYSCALL_STACK_PEAK[index].fetch_max(bytes, Ordering::Relaxed);
    }
    #[cfg(not(feature = "stack-watermark"))]
    let _ = (top, via_int80);
}

#[cfg(all(target_os = "none", feature = "stack-watermark"))]
static SYSCALL_STACK_SAMPLES: [AtomicUsize; 2] = [const { AtomicUsize::new(0) }; 2];
#[cfg(all(target_os = "none", feature = "stack-watermark"))]
static SYSCALL_STACK_PEAK: [AtomicUsize; 2] = [const { AtomicUsize::new(0) }; 2];

#[cfg(target_os = "none")]
pub(crate) fn process_probe_result() -> u64 {
    let ap_mask = secondary_cpu_mask();
    let mut vm_stress_passed = false;
    let wait_ticks = if cfg!(feature = "smp-stress-test") {
        4500
    } else {
        1000
    };
    for _ in 0..wait_ticks {
        if AP_VM_STRESS_MUNMAPS
            .iter()
            .all(|count| count.load(Ordering::Acquire) >= AP_VM_STRESS_TARGET)
        {
            vm_stress_passed = true;
            break;
        }
        if !idt::wait_ticks(1) {
            break;
        }
    }
    AP_CPL3_STOP.store(
        !cfg!(feature = "smp-stress-test") || !vm_stress_passed,
        Ordering::Release,
    );
    let mut ap_probe_passed = false;
    for _ in 0..200 {
        let active = AP_CPL3_CPU_MASK.load(Ordering::Acquire);
        let idle = AP_CPL3_IDLE_MASK.load(Ordering::Acquire);
        if vm_stress_passed
            && AP_CPL3_OVERLAP_PASSED.load(Ordering::Acquire)
            && active.count_ones() >= 2
            // Only the APs that ran these tasks have an idle transition to
            // report. The reaper barrier checks every online AP separately.
            && idle & active == active
            && AP_CPL3_RUNNING_MASK.load(Ordering::Acquire) & ap_mask == 0
        {
            ap_probe_passed = true;
            break;
        }
        if !idt::wait_ticks(1) {
            break;
        }
    }
    let vm_pages = PROCESS_PROBE_IDS
        .each_ref()
        .map(|id| process_probe_vm_memory(id.load(Ordering::Acquire)));
    let memory_accounting_passed = vm_pages.iter().enumerate().all(|(index, usage)| {
        usage.is_some_and(|usage| {
            let expected_pages = if cfg!(feature = "disk-exec-test") && index == 0 {
                // The replacement has four stack pages plus the ELF segments.
                usage.owned_leaf_pages >= 5 && usage.private_page_table_pages >= 4
            } else {
                usage.owned_leaf_pages == 2 && usage.private_page_table_pages == 4
            };
            expected_pages
                && usage.total_owned_pages
                    == usage.owned_leaf_pages + usage.private_page_table_pages
                && usage.owned_leaf_pages <= usage.leaf_page_limit
                && usage.total_owned_pages <= usage.maximum_owned_pages
        })
    });
    ap_probe_passed &= memory_accounting_passed;
    if cfg!(feature = "smp-stress-test") {
        let faults_match = PROCESS_PROBE_IDS[1..].iter().all(|id| {
            PROCESS_SCHEDULER
                .process(id.load(Ordering::Acquire))
                .is_some_and(|process| {
                    matches!(
                        process.state,
                        carlio_os::process::ProcessState::Faulted {
                            vector: 14,
                            error_code: 4,
                            address: 0
                        }
                    )
                })
        });
        ap_probe_passed &= faults_match;
        if ap_probe_passed {
            write("SMP CPL3: heap global concorrente e dois page faults isolados confirmados.\r\n");
        }
    }
    if !ap_probe_passed {
        write("SMP CPL3: stress/overlap/retorno a idle incompletos; mmap/munmap AP1=");
        write_usize(AP_VM_STRESS_MUNMAPS[0].load(Ordering::Acquire));
        write(", AP2=");
        write_usize(AP_VM_STRESS_MUNMAPS[1].load(Ordering::Acquire));
        write(".\r\n");
        for _ in 0..200 {
            if AP_CPL3_IDLE_MASK.load(Ordering::Acquire) & ap_mask == ap_mask
                && AP_CPL3_RUNNING_MASK.load(Ordering::Acquire) & ap_mask == 0
            {
                break;
            }
            if !idt::wait_ticks(1) {
                break;
            }
        }
    }
    PROCESS_PROBE_DONE.store(true, Ordering::Release);
    let switches = PROCESS_PROBE_SWITCHES.load(Ordering::Acquire);
    let cr3_switches = PROCESS_PROBE_CR3_SWITCHES.load(Ordering::Acquire);
    let faults = PROCESS_PROBE_FAULTS.load(Ordering::Acquire);
    write("Runtime: tarefas encerradas; trocas de timer=");
    write_u64(switches);
    write(", trocas de CR3=");
    write_u64(cr3_switches);
    write(", faults isolados=");
    write_u64(faults as u64);
    write(".\r\n");
    #[cfg(feature = "stack-watermark")]
    {
        for (index, entry) in ["SYSCALL", "int 0x80"].into_iter().enumerate() {
            write("Pilha kernel observada após ");
            write(entry);
            write(" (marca acumulada): ");
            let peak = SYSCALL_STACK_PEAK[index].load(Ordering::Acquire);
            write_usize(peak);
            write(" / ");
            write_usize(PROCESS_KERNEL_STACK_SIZE);
            write(" bytes; amostras=");
            let samples = SYSCALL_STACK_SAMPLES[index].load(Ordering::Acquire);
            write_usize(samples);
            write(".\r\n");
            ap_probe_passed &= samples != 0 && peak < PROCESS_KERNEL_STACK_SIZE;
        }
    }
    if ap_probe_passed {
        write(
            "SMP CPL3: mmap/munmap concluídos nos dois APs; sobreposição simultânea e retorno a idle confirmados.\r\n",
        );
        write("SMP CPL3 ciclos: AP1=");
        write_usize(AP_VM_STRESS_MUNMAPS[0].load(Ordering::Acquire));
        write(", AP2=");
        write_usize(AP_VM_STRESS_MUNMAPS[1].load(Ordering::Acquire));
        write(".\r\n");
    }
    write("Memória user por processo (folhas/tabelas/total/limite-folhas/limite-total):");
    for usage in vm_pages {
        match usage {
            Some(usage) => {
                write(" ");
                write_usize(usage.owned_leaf_pages);
                write("/");
                write_usize(usage.private_page_table_pages);
                write("/");
                write_usize(usage.total_owned_pages);
                write("/");
                write_usize(usage.leaf_page_limit);
                write("/");
                write_usize(usage.maximum_owned_pages);
            }
            None => write(" indisponível"),
        }
    }
    write(".\r\n");
    if LINUX_GETPID_PROBE_PASSED.load(Ordering::Acquire) {
        write("ABI Linux x86_64: getpid/exit via SYSCALL passou.\r\n");
    } else {
        write("ABI Linux x86_64: chamada CPL3 não passou.\r\n");
    }
    let abi_probe_mask = LINUX_CREDENTIAL_PROBE_MASK.load(Ordering::Acquire);
    if abi_probe_mask & 0x1f == 0x1f {
        write("ABI Linux: getuid/geteuid/getgid/getegid/getgroups via SYSCALL passaram.\r\n");
    } else {
        write("ABI Linux: probe CPL3 de credenciais incompleto; máscara=");
        write_hex_u64(abi_probe_mask as u64);
        write(".\r\n");
    }
    if abi_probe_mask & 0x600 == 0x600 {
        write("ABI Linux: gettid e clock_gettime monotônico CPL3 passaram.\r\n");
    } else {
        write("ABI Linux: probe CPL3 de gettid/clock_gettime incompleto.\r\n");
    }
    if abi_probe_mask & 0x20 != 0 {
        write("ABI Linux: write CPL3 validou buffer de usuário e serial.\r\n");
    } else {
        write("ABI Linux: probe CPL3 de write não passou.\r\n");
    }
    if abi_probe_mask & 0x40 != 0 {
        write("ABI Linux: read CPL3 recusou destino inválido antes da leitura serial.\r\n");
    } else {
        write("ABI Linux: probe CPL3 de read não passou.\r\n");
    }
    if abi_probe_mask & 0x80 != 0 {
        write("ABI Linux: openat/read/close CPL3 leu arquivo pelo VFS.\r\n");
    } else {
        write("ABI Linux: leitura de arquivo CPL3 pelo VFS não passou.\r\n");
    }
    if LINUX_VFS_DENIAL_PROBE_PASSED.load(Ordering::Acquire) {
        write("Segurança CPL3: openat negou arquivo 0600 de outro UID com EACCES.\r\n");
    } else {
        write("Segurança CPL3: negação POSIX de openat não passou.\r\n");
        write("ABI diagnóstico: retorno de openat negado=");
        write_hex_u64(LINUX_VFS_DENIAL_RESULT.load(Ordering::Acquire));
        write(".\r\n");
    }
    if abi_probe_mask & 0x100 != 0 {
        write("ABI Linux: read(0) CPL3 recebeu entrada serial.\r\n");
    } else {
        write("ABI Linux: read(0) CPL3 não recebeu entrada serial.\r\n");
    }
    let memory_probe_mask = LINUX_MEMORY_PROBE_MASK.load(Ordering::Acquire);
    if memory_probe_mask & 0x1f == 0x1f {
        write(
            "ABI Linux: mmap/mprotect/munmap/brk CPL3 validaram ownership e permissões de página.\r\n",
        );
    } else {
        write("ABI Linux: probe CPL3 de mmap/munmap incompleto; máscara=");
        write_hex_u64(memory_probe_mask as u64);
        write(", munmap=");
        write_hex_u64(LINUX_MUNMAP_RESULT.load(Ordering::Acquire));
        write(".\r\n");
    }
    if switches > 0
        && ap_probe_passed
        && LINUX_GETPID_PROBE_PASSED.load(Ordering::Acquire)
        && abi_probe_mask & 0x7ff == 0x7ff
        && memory_probe_mask & 0x1f == 0x1f
    {
        0xfff
    } else {
        0x7ff
    }
}

#[cfg(target_os = "none")]
pub(crate) fn process_probe_is_active() -> bool {
    PROCESS_PROBE_STARTED.load(Ordering::Acquire)
}

#[cfg(target_os = "none")]
pub(crate) fn process_probe_is_done() -> bool {
    PROCESS_PROBE_DONE.load(Ordering::Acquire)
}

#[cfg(target_os = "none")]
fn reap_terminal_processes() -> bool {
    let mut success = true;
    loop {
        let mut progressed = false;
        for id in PROCESS_SCHEDULER.reapable_ids().into_iter().flatten() {
            let result = PROCESS_SCHEDULER.reap_with(id, &PROCESS_KERNEL_STACKS, |process| {
                if let Some(bundle) = process.resources {
                    if bundle.page_table != process.page_table {
                        return Err(());
                    }
                    let storage = process_vm_slot(bundle.slot).ok_or(())?;
                    if !destroy_process_vm(storage, bundle.page_table, Some(process.id)) {
                        return Err(());
                    }
                } else if !process.is_thread() {
                    return Err(());
                }
                if !process.is_thread() {
                    OPEN_FILES.lock().close_process(process.id);
                    NETWORK_SOCKETS.lock().close_process(process.id);
                }
                Ok(())
            });
            if result.is_ok() {
                for probe_id in &PROCESS_PROBE_IDS {
                    let _ = probe_id.compare_exchange(id, 0, Ordering::AcqRel, Ordering::Acquire);
                }
                progressed = true;
            } else {
                write("Reaper: cleanup do processo falhou; recursos preservados para retry.\r\n");
                success = false;
            }
        }
        if !progressed || !success {
            break;
        }
    }
    success
}

#[cfg(target_os = "none")]
fn quiesce_process_probe_aps() -> bool {
    let kernel_root = PROCESS_KERNEL_CR3.load(Ordering::Acquire);
    let bsp_root: u64;
    // SAFETY: o BSP lê seu CR3 atual enquanto executa código e stack do kernel.
    unsafe { asm!("mov {}, cr3", out(reg) bsp_root, options(nomem, nostack, preserves_flags)) };
    if kernel_root == 0 || bsp_root & 0x000f_ffff_ffff_f000 != kernel_root {
        write("Reaper SMP: BSP não confirmou CR3 kernel; liberação cancelada.\r\n");
        return false;
    }
    // APs ainda marcados em CPL3 não causam cancelamento imediato: o epoch
    // abaixo os mantém no scheduler até cada um trocar stack/CR3 e confirmar.
    let Some(epoch) = PROCESS_REAP_BARRIER.begin() else {
        return false;
    };
    let required_aps = match u64::try_from(secondary_cpu_mask()) {
        Ok(mask) => mask,
        Err(_) => {
            let _ = PROCESS_REAP_BARRIER.release(epoch);
            return false;
        }
    };
    for _ in 0..200 {
        let all_quiescent = PROCESS_REAP_BARRIER.complete(epoch, required_aps)
            && AP_CPL3_RUNNING_MASK.load(Ordering::Acquire) & secondary_cpu_mask() == 0;
        if all_quiescent {
            let acknowledgements = required_aps.count_ones();
            write("Reaper SMP: BSP e APs confirmaram CR3 kernel; ACKs dos APs=");
            write_u64(u64::from(acknowledgements));
            write("; CPL3 ativo=0; stacks, VM e page tables liberáveis.\r\n");
            return true;
        }
        if !idt::wait_ticks(1) {
            let _ = PROCESS_REAP_BARRIER.release(epoch);
            return false;
        }
    }
    let _ = PROCESS_REAP_BARRIER.release(epoch);
    false
}

#[cfg(target_os = "none")]
fn release_process_probe_aps() {
    if let Some(epoch) = PROCESS_REAP_BARRIER.requested() {
        let _ = PROCESS_REAP_BARRIER.release(epoch);
    }
}

#[cfg(target_os = "none")]
fn destroy_process_vm(
    storage: &IrqSpinLock<Option<ProcessVmSlot>>,
    expected_root: u64,
    expected_owner: Option<u64>,
) -> bool {
    let mut guard = lock_process_vm_slot(storage);
    if guard.as_ref().is_none_or(|entry| {
        entry.value.root() != expected_root
            || expected_owner.is_some_and(|id| entry.owner != Some(id))
    }) {
        return false;
    }
    if let Some(entry) = guard
        .as_mut()
        .filter(|entry| entry.value.swapped_address().is_some())
    {
        let mut swap = lock_swap();
        let Some((owner, swap)) = entry.owner.zip(swap.as_mut()) else {
            return false;
        };
        if entry.value.discard_swap(owner, swap).is_err() {
            return false;
        }
    }
    let Some(ProcessVmSlot { owner, value: vm }) = guard.take() else {
        return false;
    };
    match vm.try_destroy() {
        Ok(_access) => true,
        Err((vm, _error)) => {
            write("Reaper: teardown do espaço de usuário recusado.\r\n");
            *guard = Some(ProcessVmSlot { owner, value: vm });
            false
        }
    }
}

#[cfg(target_os = "none")]
fn destroy_process_probe_space_by_root(expected_root: u64) -> bool {
    PROCESS_SCHEDULER
        .owned_resources()
        .iter()
        .any(|storage| destroy_process_vm(storage, expected_root, None))
}

#[cfg(target_os = "none")]
fn abort_unstarted_process_probe(ids: [u64; 3], roots: [u64; 3]) -> bool {
    let mut success = true;
    for index in 0..3 {
        if ids[index] != 0 {
            if PROCESS_SCHEDULER.destroy(ids[index], -1).is_err() {
                success = false;
                continue;
            }
            let result =
                PROCESS_SCHEDULER.reap_with(ids[index], &PROCESS_KERNEL_STACKS, |process| {
                    if process.page_table == roots[index]
                        && destroy_process_probe_space_by_root(roots[index])
                    {
                        Ok(())
                    } else {
                        Err(())
                    }
                });
            if result.is_err() {
                PROCESS_PROBE_IDS[index].store(ids[index], Ordering::Release);
                success = false;
            } else {
                PROCESS_PROBE_IDS[index].store(0, Ordering::Release);
            }
        } else if roots[index] != 0 {
            if !destroy_process_probe_space_by_root(roots[index]) {
                success = false;
            }
        }
    }
    if !success {
        write("Runtime: rollback da fixture CPL3 incompleto.\r\n");
    }
    success
}

#[cfg(target_os = "none")]
pub(crate) fn ring3_kernel_routine_selftest() -> u64 {
    use carlio_os::{
        elf::{ElfError, ElfImage},
        process::{ProcessState, Scheduler},
        user_memory::{self, UserBufferError},
    };

    const ELF_ADDRESS: u64 = 0x0000_4000_0000_0100;
    const STACK_ADDRESS: u64 = 0x0000_4000_0000_1ff0;
    const UNMAPPED_ADDRESS: u64 = 0x0000_4000_0000_2000;
    const ELF_LENGTH: usize = 120;
    let mut results = 0u64;
    let mut set = |bit: u32, passed: bool| {
        if passed {
            results |= 1 << bit;
        }
    };

    let hhdm = unsafe { core::ptr::addr_of!(HHDM_OFFSET).read_volatile() };
    let mut root: u64;
    unsafe { asm!("mov {}, cr3", out(reg) root, options(nomem, nostack, preserves_flags)) };
    root &= 0x000f_ffff_ffff_f000;
    let Ok(lookup) = PageTableManager::new(BootPageTableAccess { hhdm }, root) else {
        return 0;
    };

    let mut elf_bytes = [0u8; ELF_LENGTH];
    let copied = unsafe { user_memory::copy_from_user(&lookup, ELF_ADDRESS, &mut elf_bytes) };
    set(0, copied.is_ok());
    set(1, ElfImage::<4>::parse(&elf_bytes).is_ok());
    let mut malformed = elf_bytes;
    malformed[68..72].copy_from_slice(&7u32.to_le_bytes());
    set(
        2,
        ElfImage::<4>::parse(&malformed) == Err(ElfError::InvalidSegment),
    );
    set(
        3,
        user_memory::validate(&lookup, ELF_ADDRESS, ELF_LENGTH, false).is_ok(),
    );
    set(
        4,
        user_memory::validate(&lookup, ELF_ADDRESS, 1, true) == Err(UserBufferError::ReadOnly),
    );
    set(
        5,
        user_memory::validate(&lookup, UNMAPPED_ADDRESS, 1, false)
            == Err(UserBufferError::Unmapped),
    );
    set(
        6,
        user_memory::validate(&lookup, u64::MAX - 2, 8, false)
            == Err(UserBufferError::AddressOverflow),
    );
    let payload = [0x43, 0x41, 0x52, 0x4c, 0x49, 0x4f];
    let write_ok = unsafe { user_memory::copy_to_user(&lookup, STACK_ADDRESS, &payload) };
    let mut returned = [0u8; 6];
    let read_back = unsafe { user_memory::copy_from_user(&lookup, STACK_ADDRESS, &mut returned) };
    set(
        7,
        write_ok.is_ok() && read_back.is_ok() && returned == payload,
    );
    set(
        8,
        unsafe { user_memory::copy_to_user(&lookup, ELF_ADDRESS, &payload) }
            == Err(UserBufferError::ReadOnly),
    );
    set(
        9,
        unsafe { user_memory::copy_from_user(&lookup, UNMAPPED_ADDRESS, &mut returned) }
            == Err(UserBufferError::Unmapped),
    );

    let scheduler = Scheduler::<4, 2>::new();
    let first = scheduler.create(root, ELF_ADDRESS, STACK_ADDRESS).ok();
    let second = scheduler.create(root, ELF_ADDRESS, STACK_ADDRESS).ok();
    let first_id = first.unwrap_or(0);
    let second_id = second.unwrap_or(0);
    let schedule0 = scheduler.schedule(0).ok().flatten();
    let schedule1 = scheduler.schedule(1).ok().flatten();
    let rotate0 = scheduler.schedule(0).ok().flatten();
    let block = scheduler.block(0).is_ok();
    let wake = scheduler.wake(first_id).is_ok();
    let resume0 = scheduler.schedule(0).ok().flatten();
    let exit = scheduler.exit(0, 23).is_ok();
    let exited =
        scheduler.process(first_id).map(|p| p.state) == Some(ProcessState::Exited { code: 23 });
    let invalid_cpu = scheduler.schedule(2).is_err();
    set(
        10,
        first_id != 0
            && second_id != 0
            && schedule0.map(|p| p.id) == Some(first_id)
            && schedule1.map(|p| p.id) == Some(second_id)
            && rotate0.map(|p| p.id) == Some(first_id)
            && block
            && wake
            && resume0.map(|p| p.id) == Some(first_id)
            && exit
            && exited
            && invalid_cpu,
    );
    drop(set);
    results
}

#[cfg(target_os = "none")]
fn prepare_ring3_elf_fixture(bytes: &mut [u8; 120]) {
    bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    bytes[16..18].copy_from_slice(&2u16.to_le_bytes());
    bytes[18..20].copy_from_slice(&62u16.to_le_bytes());
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[24..32].copy_from_slice(&0x0040_0000u64.to_le_bytes());
    bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
    bytes[52..54].copy_from_slice(&64u16.to_le_bytes());
    bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
    bytes[56..58].copy_from_slice(&1u16.to_le_bytes());
    bytes[64..68].copy_from_slice(&1u32.to_le_bytes());
    bytes[68..72].copy_from_slice(&5u32.to_le_bytes());
    bytes[72..80].copy_from_slice(&120u64.to_le_bytes());
    bytes[80..88].copy_from_slice(&0x0040_0000u64.to_le_bytes());
    bytes[104..112].copy_from_slice(&16u64.to_le_bytes());
}

#[cfg(target_os = "none")]
fn record_error(error: KernelError) {
    if let Some(mut log) = ERROR_LOG.try_lock() {
        log.record(error);
    }
    if !matches!(
        error,
        KernelError::SerialReadTimeout | KernelError::SerialWriteTimeout
    ) {
        if let Some(mut pending) = PENDING_ERRORS.try_lock() {
            pending.record(error);
        }
    }
}

#[cfg(target_os = "none")]
fn initialize_persistent_storage() {
    use carlio_os::{persistent_log::PersistentLog, storage_reserve::StorageReserve};

    let Ok(mut device) = ahci::block_device() else {
        return;
    };
    let Ok(Some(reserve)) = StorageReserve::detect(&mut device) else {
        return;
    };
    if reserve.has_vfs() {
        initialize_vfs_store(reserve);
    }
    if let Ok(log) = reserve.log(device).and_then(|region| {
        PersistentLog::open(region).map_err(|_| carlio_os::block::BlockError::Device)
    }) {
        *PERSISTENT_ERRORS.lock() = Some(log);
        write("Log persistente: reserva do coletor aberta.\r\n");
    } else {
        write("Log persistente: reserva do coletor recusada.\r\n");
    }

    let Some((key, session)) = entropy::swap_credentials() else {
        return;
    };
    if let Ok(region) = ahci::block_device().and_then(|device| {
        reserve
            .swap(device)
            .map_err(|_| KernelError::DeviceInitFailed)
    }) {
        use carlio_os::block::BlockDevice;
        let count = (region.sector_count() / 8).min(64) as usize;
        let slots = alloc::boxed::Box::leak(
            alloc::vec![carlio_os::swap::SwapSlot::default(); count].into_boxed_slice(),
        );
        if let Ok(swap) = KernelSwap::new(region, key, session, slots) {
            *BOOT_SWAP.lock() = Some(swap);
        }
    }
}

#[cfg(target_os = "none")]
fn flush_persistent_errors() {
    let failed = {
        let mut storage = PERSISTENT_ERRORS.lock();
        let Some(log) = storage.as_mut() else {
            return;
        };
        let pending = core::mem::take(&mut *PENDING_ERRORS.lock());
        let failed = (0..pending.len()).rev().any(|index| {
            pending
                .recent(index)
                .is_some_and(|error| log.append(error.message().as_bytes()).is_err())
        });
        if failed {
            *storage = None;
        }
        failed
    };
    if failed {
        write("Log persistente: falha de I/O; coleta em disco suspensa.\r\n");
    }
}

#[cfg(target_os = "none")]
fn report_errors() {
    let log = *ERROR_LOG.lock();
    write("Erros registrados: ");
    write_u32(log.total());
    write("\r\n");
    for index in 0..log.len() {
        if let Some(error) = log.recent(index) {
            write("- ");
            write(error.message());
            write("\r\n");
        }
    }
}

#[cfg(target_os = "none")]
fn report_errors_markdown() {
    let log = *ERROR_LOG.lock();
    write("# Eventos do kernel (buffer volátil)\r\n\r\n");
    write("| Métrica | Valor | Gráfico |\r\n|---|---:|---|\r\n");
    write("| Eventos desde o boot | ");
    write_u32(log.total());
    write(" | ");
    let bars = log.total().min(24);
    for _ in 0..bars {
        write("#");
    }
    write(" |\r\n\r\n| Índice recente | Código | Evento |\r\n|---:|---:|---|\r\n");
    for index in 0..log.len() {
        if let Some(error) = log.recent(index) {
            write("| ");
            write_u32(index as u32);
            write(" | ");
            write_u32(error as u16 as u32);
            write(" | ");
            write(error.message());
            write(" |\r\n");
        }
    }
    write("\r\nAviso: captura em RAM, limitada e perdida ao reiniciar; não é log persistente.\r\n");
}

#[cfg(target_os = "none")]
fn report_memory_map() {
    let hhdm = unsafe { core::ptr::addr_of!(HHDM_REQUEST.response).read_volatile() };
    let map = unsafe { core::ptr::addr_of!(MEMMAP_REQUEST.response).read_volatile() };
    let kernel = unsafe { core::ptr::addr_of!(KERNEL_ADDRESS_REQUEST.response).read_volatile() };
    if hhdm.is_null() || map.is_null() || kernel.is_null() {
        record_error(KernelError::BootInfoMissing);
        write(
            "Aviso: mapa/HHDM/endereço do kernel indisponível; drivers DMA/MMIO desativados.\r\n",
        );
        return;
    }
    let offset = unsafe { (*hhdm).offset };
    let entry_count = unsafe { (*map).entry_count.min(4096) };
    let entries = unsafe { (*map).entries };
    if entries.is_null() {
        record_error(KernelError::BootInfoMissing);
        write("Aviso: lista do mapa de memória inválida; drivers DMA/MMIO desativados.\r\n");
        return;
    }
    let mut usable_bytes = 0u64;
    let mut usable_regions = 0u32;
    for index in 0..entry_count as usize {
        let entry = unsafe { entries.add(index).read_volatile() };
        if entry.is_null() {
            continue;
        }
        let entry = unsafe { &*entry };
        if entry.kind == 0 {
            usable_regions = usable_regions.saturating_add(1);
            usable_bytes = usable_bytes.saturating_add(entry.length);
            if FRAME_ALLOCATOR
                .lock()
                .add_usable_region(entry.base, entry.length)
                .is_err()
            {
                record_error(KernelError::BootInfoMissing);
            }
        }
    }
    unsafe {
        core::ptr::addr_of_mut!(HHDM_OFFSET).write_volatile(offset);
        core::ptr::addr_of_mut!(KERNEL_PHYSICAL_BASE).write_volatile((*kernel).physical_base);
        core::ptr::addr_of_mut!(KERNEL_VIRTUAL_BASE).write_volatile((*kernel).virtual_base);
    }
    write("Mapa de memória Limine: regiões usáveis=");
    write_u32(usable_regions);
    write(", RAM usável MiB=");
    write_u32((usable_bytes / (1024 * 1024)).min(u32::MAX as u64) as u32);
    write("; HHDM e bases físico/virtual do kernel disponíveis.\r\n");
    write("Allocator físico: frames livres até 32 GiB=");
    write_u64(FRAME_ALLOCATOR.lock().free_frames() as u64);
    let high_memory_probe = FRAME_ALLOCATOR
        .lock()
        .allocate_at_or_above(4 * 1024 * 1024 * 1024);
    let probe_frame = high_memory_probe.or_else(|| FRAME_ALLOCATOR.lock().allocate());
    if high_memory_probe.is_some() {
        write(", frame acima de 4 GiB alocado e testado");
    } else {
        write(", sem RAM acima de 4 GiB; teste usa frame baixo");
    }
    let probe_ok = if let Some(physical) = probe_frame {
        let probe_address = offset.checked_add(physical);
        if let Some(virtual_address) = probe_address {
            let pointer = virtual_address as *mut u64;
            unsafe {
                core::ptr::write_volatile(pointer, 0x4341_524c_494f_4f53);
                let valid = core::ptr::read_volatile(pointer) == 0x4341_524c_494f_4f53;
                core::ptr::write_volatile(pointer, 0);
                valid
            }
        } else {
            false
        }
    } else {
        false
    };
    if let Some(physical) = probe_frame {
        let _ = FRAME_ALLOCATOR.lock().free(physical);
    }
    if probe_ok {
        write("; alocação, escrita, leitura e liberação passaram.\r\n");
    } else {
        record_error(KernelError::DeviceInitFailed);
        write("; teste de acesso ao frame falhou.\r\n");
    }
    if probe_ok {
        PAGE_TABLE_PROBE_PENDING.store(true, Ordering::Release);
    }
}

#[cfg(target_os = "none")]
fn page_table_boot_probe(hhdm: u64) {
    const TEST_VIRTUAL: u64 = 0xffff_a000_0000_0000;
    const TEST_USER_VIRTUAL: u64 = 0x0000_4000_0000_0000;
    let frame = FRAME_ALLOCATOR.lock().allocate();
    let Some(physical) = frame else {
        write("VM: sem frame para o teste de mapeamento.\r\n");
        return;
    };
    let root: u64;
    unsafe { asm!("mov {}, cr3", out(reg) root, options(nomem, nostack, preserves_flags)) };
    let root = root & 0x000f_ffff_ffff_f000;
    let access = BootPageTableAccess { hhdm };
    let mut tables = match PageTableManager::new(access, root) {
        Ok(manager) => manager,
        Err(_) => {
            let _ = FRAME_ALLOCATOR.lock().free(physical);
            write("VM: CR3 inválido; teste de paginação não executado.\r\n");
            return;
        }
    };
    let result = tables.map(TEST_VIRTUAL, physical, 0x3).and_then(|()| {
        let pointer = TEST_VIRTUAL as *mut u64;
        unsafe { core::ptr::write_volatile(pointer, 0x5041_4745_5441_424c) };
        let valid = unsafe { core::ptr::read_volatile(pointer) == 0x5041_4745_5441_424c };
        let unmapped = tables.unmap(TEST_VIRTUAL)? == physical;
        if !valid || !unmapped {
            return Err(FrameError::CorruptTable);
        }
        tables.map_user(TEST_USER_VIRTUAL, physical, 0x3)?;
        let user_mapping = tables.mapping(TEST_USER_VIRTUAL)?;
        let user_pointer = TEST_USER_VIRTUAL as *mut u64;
        unsafe { core::ptr::write_volatile(user_pointer, 0x5553_4552_5041_4745) };
        let user_access_valid =
            unsafe { core::ptr::read_volatile(user_pointer) == 0x5553_4552_5041_4745 }
                && user_mapping.flags & PAGE_FLAG_USER != 0;
        let user_unmapped = tables.unmap(TEST_USER_VIRTUAL)? == physical;
        if user_access_valid && user_unmapped {
            Ok(())
        } else {
            Err(FrameError::CorruptTable)
        }
    });
    let address_space_result = if result.is_ok() {
        (|| {
            const USER_STACK_VIRTUAL: u64 = TEST_USER_VIRTUAL + 4096;
            const USER_STACK_TOP: u64 = USER_STACK_VIRTUAL + 4096;
            const USER_PROBE_CODE: [u8; 7] = [
                0xb8, 0xed, 0xfe, 0x00, 0x00, // mov eax, SYS_KERNEL_ROUTINE_PROBE
                0xcd, 0x80, // int 0x80; kernel executa a bateria de prova
            ];
            let stack_physical = FRAME_ALLOCATOR
                .lock()
                .allocate()
                .ok_or(FrameError::TableAllocationFailed)?;
            let code_pointer =
                hhdm.checked_add(physical)
                    .ok_or(FrameError::InvalidPhysicalAddress)? as *mut u8;
            let stack_pointer =
                hhdm.checked_add(stack_physical)
                    .ok_or(FrameError::InvalidPhysicalAddress)? as *mut u8;
            unsafe {
                core::ptr::write_bytes(code_pointer, 0, 4096);
                core::ptr::copy_nonoverlapping(
                    USER_PROBE_CODE.as_ptr(),
                    code_pointer,
                    USER_PROBE_CODE.len(),
                );
                let mut elf_fixture = [0u8; 120];
                prepare_ring3_elf_fixture(&mut elf_fixture);
                core::ptr::copy_nonoverlapping(
                    elf_fixture.as_ptr(),
                    code_pointer.add(0x100),
                    elf_fixture.len(),
                );
                core::ptr::write_bytes(stack_pointer, 0, 4096);
            }

            let mut space = match OwnedAddressSpace::<_, 2>::new(BootPageTableAccess { hhdm }, root)
            {
                Ok(space) => space,
                Err(error) => {
                    let _ = FRAME_ALLOCATOR.lock().free(stack_physical);
                    return Err(error);
                }
            };
            let mut code_transferred = false;
            let mut stack_transferred = false;
            let execution = (|| {
                space.map_owned_frame(TEST_USER_VIRTUAL, physical, 0x1)?;
                code_transferred = true;
                space.map_owned_frame(USER_STACK_VIRTUAL, stack_physical, 0x3)?;
                stack_transferred = true;
                let code_mapping = space.mapping(TEST_USER_VIRTUAL)?;
                let stack_mapping = space.mapping(USER_STACK_VIRTUAL)?;
                let kernel_cannot_see_user_va =
                    PageTableManager::new(BootPageTableAccess { hhdm }, root)?
                        .mapping(TEST_USER_VIRTUAL)
                        == Err(FrameError::NotMapped);
                let old_cr3: u64;
                unsafe {
                    asm!("mov {}, cr3", out(reg) old_cr3, options(nomem, nostack, preserves_flags))
                };
                PROCESS_KERNEL_CR3.store(old_cr3 & 0x000f_ffff_ffff_f000, Ordering::Release);
                let process_cr3 = space.root();
                unsafe {
                    asm!("mov cr3, {}", in(reg) process_cr3, options(nostack, preserves_flags))
                };
                unsafe { idt::start_watchdog() };
                let free_frames_before = FRAME_ALLOCATOR.lock().free_frames();
                let routine_results =
                    unsafe { idt::run_ring3_probe(TEST_USER_VIRTUAL, USER_STACK_TOP) };
                unsafe {
                    asm!("mov cr3, {}", in(reg) process_cr3, options(nostack, preserves_flags))
                };
                unsafe { idt::restore_bootstrap_kernel_stack() };
                let entered = routine_results & 0x7ff == 0x7ff;
                unsafe { asm!("mov cr3, {}", in(reg) old_cr3, options(nostack, preserves_flags)) };
                let quiescent = quiesce_process_probe_aps();
                if quiescent {
                    PROCESS_PROBE_STARTED.store(false, Ordering::Release);
                }
                let reaped = quiescent
                    && reap_terminal_processes()
                    && PROCESS_PROBE_IDS
                        .iter()
                        .all(|id| id.load(Ordering::Acquire) == 0);
                let frames_restored = if reaped {
                    let free_frames_after = FRAME_ALLOCATOR.lock().free_frames();
                    if free_frames_after == free_frames_before {
                        write("Allocator por processo: frames livres restaurados ao baseline=");
                        write_usize(free_frames_after);
                        write(".\r\n");
                        true
                    } else {
                        write("Allocator por processo: divergência de frames; baseline=");
                        write_usize(free_frames_before);
                        write(", atual=");
                        write_usize(free_frames_after);
                        write(".\r\n");
                        false
                    }
                } else {
                    false
                };
                let preempted = routine_results == 0xfff && frames_restored;
                if quiescent {
                    release_process_probe_aps();
                }
                unsafe {
                    asm!("mov cr3, {}", in(reg) process_cr3, options(nostack, preserves_flags))
                };
                unsafe {
                    core::ptr::write_volatile(code_pointer, 0x0f);
                    core::ptr::write_volatile(code_pointer.add(1), 0x0b);
                }
                let fault_result =
                    unsafe { idt::run_ring3_probe(TEST_USER_VIRTUAL, USER_STACK_TOP) };
                let fault_recovered = fault_result == (0x100 | 6);
                unsafe { asm!("mov cr3, {}", in(reg) old_cr3, options(nostack, preserves_flags)) };
                if entered
                    && preempted
                    && fault_recovered
                    && kernel_cannot_see_user_va
                    && code_mapping.physical == physical
                    && code_mapping.flags & PAGE_FLAG_USER != 0
                    && stack_mapping.physical == stack_physical
                    && stack_mapping.flags & PAGE_FLAG_USER != 0
                    && stack_mapping.flags & 2 != 0
                {
                    Ok(())
                } else {
                    write("VM debug: entered=");
                    write(if entered { "1" } else { "0" });
                    write(" preempted=");
                    write(if preempted { "1" } else { "0" });
                    write(" fault=");
                    write(if fault_recovered { "1" } else { "0" });
                    write(" kernel-isolated=");
                    write(if kernel_cannot_see_user_va { "1" } else { "0" });
                    write(" fault-result=");
                    write_hex_u64(fault_result);
                    write(".\r\n");
                    Err(FrameError::CorruptTable)
                }
            })();
            let destroyed = match space.try_destroy() {
                Ok(_access) => Ok(()),
                Err((_space, error)) => Err(error),
            };
            if !stack_transferred {
                let _ = FRAME_ALLOCATOR.lock().free(stack_physical);
            }
            if !code_transferred {
                let _ = FRAME_ALLOCATOR.lock().free(physical);
            }
            execution.and(destroyed)
        })()
    } else {
        Err(FrameError::CorruptTable)
    };
    if address_space_result.is_err() {
        // O frame pode ter sido liberado pelo espaço com ownership; o allocator
        // recusa double-free e mantém essa verificação como última proteção.
        let _ = FRAME_ALLOCATOR.lock().free(physical);
    }
    if result.is_ok() && address_space_result.is_ok() {
        write("Ring 3: probe CPL3, retorno seguro e fault #UD isolado passaram.\r\n");
        write(
            "Ring 3 rotinas: ELF carregado em CPL3, scheduler e preempção entre dois CR3 passaram; pilhas recuperadas pelo reaper.\r\n",
        );
        write("TLB SMP: shootdowns enviados=");
        write_u32(lapic::shootdown_count().min(u32::MAX as usize) as u32);
        write(", ACKs remotos por IPI concluídos antes de liberar tabelas.\r\n");
    } else {
        write("VM: teste de tabelas de página falhou; kernel manteve CR3 do Limine.\r\n");
        record_error(KernelError::DeviceInitFailed);
    }
}

#[cfg(target_os = "none")]
fn kernel_heap_boot_probe() {
    use core::alloc::Layout;
    use core::ptr::NonNull;

    let region = core::ptr::addr_of_mut!(KERNEL_HEAP_BOOT_REGION).cast::<u8>();
    {
        let mut global = KERNEL_HEAP.lock();
        if global.region_count() != 0
            || unsafe { global.add_region(NonNull::new(region).unwrap(), 64 * 1024) }.is_err()
        {
            record_error(KernelError::DeviceInitFailed);
            write("Heap kernel: inicialização da região de bootstrap falhou.\r\n");
            return;
        }
    }
    let layout = Layout::from_size_align(257, 64).unwrap();
    let Some(allocation) = kernel_heap_allocate(layout) else {
        record_error(KernelError::DeviceInitFailed);
        write("Heap kernel: alocação de prova falhou.\r\n");
        return;
    };
    unsafe {
        allocation.as_ptr().write_bytes(0xA7, layout.size());
    }
    let valid =
        unsafe { (0..layout.size()).all(|offset| allocation.as_ptr().add(offset).read() == 0xA7) };
    let released = unsafe { kernel_heap_deallocate(allocation).is_ok() };
    let mut allocations = [None; 128];
    let stress_layout = Layout::from_size_align(600, 16).unwrap();
    for item in &mut allocations {
        *item = kernel_heap_allocate(stress_layout);
    }
    let grew = KERNEL_HEAP.lock().region_count() > 1;
    let stress_ok = allocations.iter().all(Option::is_some);
    for allocation in allocations.into_iter().flatten() {
        let _ = unsafe { kernel_heap_deallocate(allocation) };
    }
    let mut large = alloc::vec::Vec::<u8>::new();
    let global_ok = large.try_reserve_exact(96 * 1024).is_ok() && {
        large.resize(96 * 1024, 0x39);
        core::hint::black_box(&large)
            .iter()
            .all(|&byte| byte == 0x39)
    };
    drop(large);
    if valid && released && stress_ok && grew && global_ok {
        write("Heap global: alocação Rust maior que a arena bootstrap passou.\r\n");
        write(
            "Heap kernel: regiões dinâmicas, alinhamento, escrita/leitura, expansão e liberação passaram.\r\n",
        );
    } else {
        record_error(KernelError::DeviceInitFailed);
        write("Heap kernel: teste de alocação/liberação falhou.\r\n");
    }
}

#[cfg(target_os = "none")]
fn heap_stress_cycle(process_id: u64) -> bool {
    let byte = process_id as u8;
    let mut allocation = alloc::vec::Vec::<u8>::new();
    if allocation.try_reserve_exact(512).is_err() {
        return false;
    }
    allocation.resize(512, byte);
    core::hint::black_box(&allocation)
        .iter()
        .all(|&value| value == byte)
}

#[cfg(target_os = "none")]
fn kernel_heap_allocate(layout: core::alloc::Layout) -> Option<core::ptr::NonNull<u8>> {
    {
        let mut heap = KERNEL_HEAP.lock();
        if heap.region_count() == 0 {
            return None;
        }
        if let Some(pointer) = heap.allocate(layout) {
            return Some(pointer);
        }
    }
    const PAGE_BYTES: usize = 4096;
    let bytes = KernelHeapSet::<256>::arena_size_for(layout)?;
    let pages = bytes.div_ceil(PAGE_BYTES);
    let arena_bytes = pages.checked_mul(PAGE_BYTES)?;
    let physical = FRAME_ALLOCATOR.lock().allocate_contiguous(pages)?;
    let hhdm = unsafe { core::ptr::addr_of!(HHDM_OFFSET).read_volatile() };
    let region = hhdm
        .checked_add(physical)
        .and_then(|address| address.checked_add(arena_bytes as u64).map(|_| address))
        .and_then(|address| core::ptr::NonNull::new(address as *mut u8));
    if let Some(region) = region {
        let mut heap = KERNEL_HEAP.lock();
        // Another CPU may have freed a block or expanded while we reserved frames.
        if let Some(pointer) = heap.allocate(layout) {
            drop(heap);
            release_heap_frames(physical, pages);
            return Some(pointer);
        }
        if unsafe { heap.add_region(region, arena_bytes) }.is_ok() {
            return heap.allocate(layout);
        }
    }
    release_heap_frames(physical, pages);
    None
}

#[cfg(target_os = "none")]
fn release_heap_frames(physical: u64, pages: usize) {
    let mut allocator = FRAME_ALLOCATOR.lock();
    for page in 0..pages {
        let _ = allocator.free(physical + page as u64 * 4096);
    }
}

#[cfg(target_os = "none")]
unsafe fn kernel_heap_deallocate(
    pointer: core::ptr::NonNull<u8>,
) -> Result<(), carlio_os::heap::HeapError> {
    KERNEL_HEAP.lock().deallocate(pointer)
}

#[cfg(target_os = "none")]
fn write_u32(mut number: u32) {
    let mut digits = [0u8; 10];
    let mut len = 0;
    loop {
        digits[len] = b'0' + (number % 10) as u8;
        len += 1;
        number /= 10;
        if number == 0 {
            break;
        }
    }
    while len > 0 {
        len -= 1;
        if !serial_write(digits[len]) {
            return;
        }
    }
}

#[cfg(target_os = "none")]
fn write_hex_u64(value: u64) {
    for shift in (0..16).rev() {
        write_hex_nibble(((value >> (shift * 4)) & 0xf) as u8);
    }
}

#[cfg(target_os = "none")]
fn write_u64(value: u64) {
    let mut digits = [0u8; 20];
    let mut len = 0;
    let mut number = value;
    loop {
        digits[len] = b'0' + (number % 10) as u8;
        len += 1;
        number /= 10;
        if number == 0 {
            break;
        }
    }
    while len > 0 {
        len -= 1;
        if !serial_write(digits[len]) {
            return;
        }
    }
}

#[cfg(target_os = "none")]
fn write_usize(value: usize) {
    match u64::try_from(value) {
        Ok(value) => write_u64(value),
        Err(_) => write("overflow"),
    }
}

#[cfg(target_os = "none")]
fn enumerate_pci() {
    write("Inventário PCI (somente leitura):\r\n");
    let found = pci_device::enumerate(report_pci_function);
    write("Funções PCI presentes detectadas: ");
    write_u32(found as u32);
    write("\r\n");
}

#[cfg(target_os = "none")]
fn initialize_vfs_mounts() {
    let root = Credentials { uid: 0, gid: 0 };
    let mut vfs = PROCESS_VFS.lock();
    if vfs
        .check_access_as(
            "/bin",
            AccessContext::new(root, &[], 0),
            carlio_os::vfs::Access::READ,
        )
        .is_err()
    {
        if vfs.create_fhs_layout().is_err() {
            write("VFS: falha ao criar hierarquia FHS em RAM.\r\n");
            return;
        }
    }
    let rootfs = ROOTFS_BACKEND.lock();
    let Some(backend) = rootfs.as_ref() else {
        write("VFS: nenhum rootfs ext4/FAT32 montável; namespace FHS em RAM.\r\n");
        return;
    };
    let filesystem = carlio_os::vfs::ReadOnlyMountBackend::filesystem_type(backend);
    match vfs.mount(
        "/dev/ahci-partition",
        "/",
        filesystem,
        carlio_os::vfs::MountOptions {
            read_only: true,
            no_exec: false,
            no_suid: false,
        },
        root,
    ) {
        Ok(()) => write("VFS: rootfs montado em / para leitura; escrita no overlay RAM.\r\n"),
        Err(_) => write("VFS: rootfs detectado, mas o mount no namespace falhou.\r\n"),
    }
}

#[cfg(target_os = "none")]
fn rootfs_metadata(
    path: &str,
    context: AccessContext<'_>,
) -> Result<carlio_os::vfs::MountMetadata, carlio_os::vfs::VfsError> {
    let vfs = PROCESS_VFS.lock();
    let mut backend = ROOTFS_BACKEND.lock();
    let backend = backend
        .as_mut()
        .ok_or(carlio_os::vfs::VfsError::NotMounted)?;
    let mount = vfs
        .mount_for(path)
        .ok_or(carlio_os::vfs::VfsError::NotMounted)?;
    let relative = if path == mount.path {
        "/"
    } else if mount.path == "/" {
        path
    } else {
        &path[mount.path.len()..]
    };
    let metadata = carlio_os::vfs::ReadOnlyMountBackend::metadata(backend, relative)?;
    let _ = context;
    Ok(metadata)
}

#[cfg(target_os = "none")]
fn read_rootfs_file(
    path: &str,
    context: AccessContext<'_>,
    offset: u64,
    output: &mut [u8],
) -> Result<usize, carlio_os::vfs::VfsError> {
    let vfs = PROCESS_VFS.lock();
    let mut backend = ROOTFS_BACKEND.lock();
    let backend = backend
        .as_mut()
        .ok_or(carlio_os::vfs::VfsError::NotMounted)?;
    let mount = vfs
        .mount_for(path)
        .ok_or(carlio_os::vfs::VfsError::NotMounted)?;
    let relative = if path == mount.path {
        "/"
    } else if mount.path == "/" {
        path
    } else {
        &path[mount.path.len()..]
    };
    let metadata = carlio_os::vfs::ReadOnlyMountBackend::metadata(backend, relative)?;
    if metadata.kind != carlio_os::vfs::NodeKind::File
        || !mount_permission(metadata, context, 0b100)
    {
        return Err(carlio_os::vfs::VfsError::PermissionDenied);
    }
    vfs.read_mounted_file_range(path, context, backend, offset, output)
}

#[cfg(target_os = "none")]
fn mount_permission(
    metadata: carlio_os::vfs::MountMetadata,
    context: AccessContext<'_>,
    requested: u8,
) -> bool {
    if context.credentials.uid == 0 {
        return requested & 1 == 0 || metadata.mode & 0o111 != 0;
    }
    let shift = if context.credentials.uid == metadata.uid {
        6
    } else if context.credentials.gid == metadata.gid
        || context.supplementary_groups.contains(&metadata.gid)
    {
        3
    } else {
        0
    };
    ((metadata.mode >> shift) as u8 & requested) == requested
}

#[cfg(target_os = "none")]
fn ensure_overlay_parent<'a, const N: usize>(
    vfs: &mut Vfs<N>,
    path: &'a str,
    context: AccessContext<'_>,
) -> Result<(&'a str, &'a str), carlio_os::vfs::VfsError> {
    let (parent, name) = path
        .rsplit_once('/')
        .ok_or(carlio_os::vfs::VfsError::InvalidPath)?;
    if name.is_empty() {
        return Err(carlio_os::vfs::VfsError::InvalidPath);
    }
    let parent = if parent.is_empty() { "/" } else { parent };
    let mut prefix = [0u8; carlio_os::vfs::VFS_PATH_MAX];
    prefix[0] = b'/';
    let mut length = 1usize;
    for component in parent.split('/').filter(|part| !part.is_empty()) {
        let mut candidate_bytes = prefix;
        let candidate_start = if length == 1 { length } else { length + 1 };
        if candidate_start > length {
            candidate_bytes[length] = b'/';
        }
        let end = candidate_start
            .checked_add(component.len())
            .ok_or(carlio_os::vfs::VfsError::InvalidPath)?;
        if end >= candidate_bytes.len() {
            return Err(carlio_os::vfs::VfsError::InvalidPath);
        }
        candidate_bytes[candidate_start..end].copy_from_slice(component.as_bytes());
        let candidate = core::str::from_utf8(&candidate_bytes[..end])
            .map_err(|_| carlio_os::vfs::VfsError::InvalidPath)?;
        if vfs
            .check_access_as(candidate, context, carlio_os::vfs::Access::READ)
            .is_err()
        {
            let parent_prefix = core::str::from_utf8(&prefix[..length])
                .map_err(|_| carlio_os::vfs::VfsError::InvalidPath)?;
            vfs.create_dir_as(parent_prefix, component, context, 0o755)?;
        }
        prefix = candidate_bytes;
        length = end;
    }
    Ok((parent, name))
}

#[cfg(target_os = "none")]
fn discover_boot_storage() {
    let Some(mut device) = AHCI_BLOCK_DEVICE.lock().take() else {
        write("Storage: nenhum dispositivo de bloco disponível para varredura.\r\n");
        return;
    };
    match carlio_os::partition::discover(&mut device) {
        Ok((partitions, count)) => {
            write("Storage: partições ext4/FAT32 detectadas: ");
            write_u32(count as u32);
            write("\r\n");
            for partition in partitions[..count].iter().flatten() {
                write("  ");
                match partition.filesystem {
                    carlio_os::partition::Filesystem::Ext4 => write("ext4"),
                    carlio_os::partition::Filesystem::Fat32 => write("FAT32"),
                }
                write(" LBA=");
                write_u64(partition.start_lba);
                write(" setores=");
                write_u64(partition.sectors);
                write("\r\n");
            }
            let chosen = partitions[..count]
                .iter()
                .flatten()
                .find(|partition| partition.filesystem == carlio_os::partition::Filesystem::Ext4)
                .or_else(|| {
                    partitions[..count].iter().flatten().find(|partition| {
                        partition.filesystem == carlio_os::partition::Filesystem::Fat32
                    })
                })
                .copied();
            if let Some(partition) = chosen {
                let backend = match carlio_os::block::BlockRegion::new(
                    device,
                    partition.start_lba,
                    partition.sectors,
                ) {
                    Err(_) => Err("partição excede os limites do dispositivo"),
                    Ok(region) => match partition.filesystem {
                        carlio_os::partition::Filesystem::Ext4 => {
                            carlio_os::ext4::Ext4Volume::mount(region, 0)
                                .map(BootRootfs::Ext4)
                                .map_err(|_| "ext4 recusado pelo leitor")
                        }
                        carlio_os::partition::Filesystem::Fat32 => {
                            carlio_os::fat32::Fat32Volume::mount(region, 0)
                                .map(BootRootfs::Fat32)
                                .map_err(|_| "FAT32 recusado pelo leitor")
                        }
                    },
                };
                match backend {
                    Ok(backend) => {
                        *ROOTFS_BACKEND.lock() = Some(backend);
                        write("Storage: partição escolhida como rootfs prioritário.\r\n");
                    }
                    Err(reason) => write(reason),
                }
            } else {
                *AHCI_BLOCK_DEVICE.lock() = Some(device);
            }
        }
        Err(error) => {
            write("Storage: descoberta de partições falhou: ");
            match error {
                carlio_os::partition::DiscoveryError::Device => {
                    write("erro de leitura do dispositivo")
                }
                carlio_os::partition::DiscoveryError::InvalidTable => write("tabela inválida"),
                carlio_os::partition::DiscoveryError::UnsupportedTable => {
                    write("formato GPT/partição não suportado")
                }
            }
            write("\r\n");
            *AHCI_BLOCK_DEVICE.lock() = Some(device);
        }
    }
}

#[cfg(target_os = "none")]
fn initialize_vfs_store(reserve: carlio_os::storage_reserve::StorageReserve) {
    let mut vfs = PROCESS_VFS.lock();
    let mut before = VFS_BEFORE.lock();
    let mut snapshot = VFS_AFTER.lock();
    let result = (|| {
        let initial = vfs.encode_snapshot(&mut *before).ok()?;
        let region = reserve.vfs(ahci::block_device().ok()?).ok()?;
        let (mut store, len) = OverlayStore::open(region, &mut *snapshot).ok()?;
        if len == 0 {
            store.commit(&before[..initial]).ok()?;
        } else if vfs.restore_overlay(&snapshot[..len]).is_err() {
            let _ = vfs.restore_overlay(&before[..initial]);
            return None;
        }
        *VFS_STORE.lock() = Some(store);
        Some(())
    })();
    if result.is_some() {
        write("VFS: overlay persistente aberto.\r\n");
    } else {
        VFS_STORE_FAILED.store(true, Ordering::Release);
        write("VFS: reserva persistente recusada; alterações bloqueadas.\r\n");
    }
}

#[cfg(target_os = "none")]
fn commit_vfs_mutation(transaction: &VfsMutationGuard) -> Result<(), u16> {
    let Some(before_len) = transaction.before else {
        return Ok(());
    };
    let vfs = PROCESS_VFS.lock();
    let mut snapshot = VFS_AFTER.lock();
    let before = VFS_BEFORE.lock();
    let length = vfs.encode_snapshot(&mut *snapshot).map_err(vfs_errno)?;
    if before[..before_len] == snapshot[..length] {
        transaction.committed.set(true);
        return Ok(());
    }
    let result = VFS_STORE
        .lock()
        .as_mut()
        .ok_or(carlio_os::block::BlockError::Device)
        .and_then(|store| store.commit(&snapshot[..length]));
    if result.is_err() {
        VFS_STORE_FAILED.store(true, Ordering::Release);
        return Err(carlio_os::linux_abi::EIO);
    }
    transaction.committed.set(true);
    Ok(())
}

#[cfg(all(target_os = "none", feature = "storage-test"))]
fn vfs_storage_probe(command: &str) -> Result<(), u16> {
    use carlio_os::linux_abi::EIO;
    let transaction = VfsMutationGuard::acquire()?;
    if transaction.before.is_none() {
        return Err(EIO);
    }
    let root = Credentials { uid: 0, gid: 0 };
    let access = AccessContext::new(root, &[], 0);
    let mut vfs = PROCESS_VFS.lock();
    let path = "/tmp/prova-persistente";
    match command {
        "vfs-teste-gravar" => {
            vfs.create_file("/tmp", "prova-persistente", root, 0o640)
                .map_err(vfs_errno)?;
            vfs.write_file_as(path, access, b"estado anterior")
                .map_err(vfs_errno)?;
        }
        "vfs-teste-alterar" => {
            vfs.set_mode_as(path, access, 0o600).map_err(vfs_errno)?;
            vfs.write_file_as(path, access, b"estado novo")
                .map_err(vfs_errno)?;
        }
        _ => {
            let mut bytes = [0; 32];
            let length = vfs
                .read_file_as(path, access, &mut bytes)
                .map_err(vfs_errno)?;
            let metadata = vfs.metadata(path, access).map_err(vfs_errno)?;
            if &bytes[..length] != b"estado anterior" || metadata.mode != 0o640 {
                return Err(EIO);
            }
        }
    }
    drop(vfs);
    commit_vfs_mutation(&transaction)?;
    match command {
        "vfs-teste-gravar" => write("VFS: primeira geração confirmada.\r\n"),
        "vfs-teste-alterar" => write("VFS: segunda geração confirmada.\r\n"),
        _ => write("VFS: dados e permissões anteriores recuperados.\r\n"),
    }
    Ok(())
}

#[cfg(target_os = "none")]
fn report_pci_function(function: PciFunction) {
    write("PCI ");
    write_hex_byte(function.bus);
    write(":");
    write_hex_byte(function.device);
    write(".");
    write_hex_nibble(function.function);
    write(" ");
    write_hex_word(function.vendor_id);
    write(":");
    write_hex_word(function.device_id);
    write(" classe=");
    write(function.class_name());
    if function.is_sata_ahci() {
        write(" SATA/AHCI");
        match function.ahci_model() {
            Some(carlio_os::pci::AhciModel::IntelRstLocal) => {
                write(" (Intel RST local: AHCI somente leitura experimental)")
            }
            Some(carlio_os::pci::AhciModel::IntelNas) => {
                write(" (Intel AHCI NAS: somente leitura experimental)")
            }
            Some(carlio_os::pci::AhciModel::IntelQemu) => {
                write(" (QEMU: reconhecido; driver AHCI experimental)")
            }
            Some(carlio_os::pci::AhciModel::Unsupported) => {
                write(" (ID não suportado; sem acesso a disco)")
            }
            None => write(" (interface AHCI incompatível; sem acesso a disco)"),
        }
    } else if function.is_usb() {
        write(" USB");
    } else if function.is_network() {
        write(" rede");
        match function.network_model() {
            Some(carlio_os::pci::NetworkModel::RealtekRtl8111Nas) => {
                write(" (Realtek RTL8111 do servidor; driver pendente)")
            }
            Some(carlio_os::pci::NetworkModel::IntelI219Local) => {
                write(" (Intel I219-LM local; driver pendente)")
            }
            Some(carlio_os::pci::NetworkModel::Intel82574Qemu) => write(" (Intel 82574L/QEMU)"),
            Some(carlio_os::pci::NetworkModel::Unsupported) => {
                write(" (controlador não suportado)")
            }
            None => {}
        }
    } else if function.is_video() {
        write(" vídeo");
        match function.integrated_graphics_model() {
            Some(carlio_os::pci::IntegratedGraphicsModel::IntelHdGraphics520) => {
                write(" (Intel HD Graphics 520");
                report_intel_framebuffer_status();
            }
            Some(carlio_os::pci::IntegratedGraphicsModel::IntelIvyBridgeGt1) => {
                write(" (Intel Ivy Bridge GT1");
                report_intel_framebuffer_status();
            }
            Some(carlio_os::pci::IntegratedGraphicsModel::UnsupportedIntel) => {
                write(" (Intel iGPU sem suporte nativo)");
            }
            None => {}
        }
    }
    write("\r\n");
    if matches!(
        function.ahci_model(),
        Some(
            carlio_os::pci::AhciModel::IntelQemu
                | carlio_os::pci::AhciModel::IntelNas
                | carlio_os::pci::AhciModel::IntelRstLocal
        )
    ) {
        let hhdm = unsafe { core::ptr::addr_of!(HHDM_OFFSET).read_volatile() };
        let physical = unsafe { core::ptr::addr_of!(KERNEL_PHYSICAL_BASE).read_volatile() };
        let virtual_base = unsafe { core::ptr::addr_of!(KERNEL_VIRTUAL_BASE).read_volatile() };
        if hhdm != 0 && virtual_base != 0 {
            match unsafe { ahci::initialize(function, hhdm, physical, virtual_base) } {
                Ok(sectors) => {
                    write("AHCI: dispositivo identificado em leitura, setores=");
                    write_u64(sectors);
                    match ahci::block_device() {
                        Ok(device) => {
                            *AHCI_BLOCK_DEVICE.lock() = Some(device);
                            write(". Adaptador BlockDevice pronto para rootfs.\r\n");
                            discover_boot_storage();
                        }
                        Err(error) => {
                            write(". Adaptador indisponivel: ");
                            write(error.message());
                            write(".\r\n");
                        }
                    }
                }
                Err(error) => {
                    write("AHCI indisponível: ");
                    write(error.message());
                    write(".\r\n");
                }
            }
        } else {
            write("AHCI não iniciado: mapa DMA indisponível.\r\n");
        }
    }
    if function.network_model() == Some(carlio_os::pci::NetworkModel::Intel82574Qemu) {
        let hhdm = unsafe { core::ptr::addr_of!(HHDM_OFFSET).read_volatile() };
        let physical = unsafe { core::ptr::addr_of!(KERNEL_PHYSICAL_BASE).read_volatile() };
        let virtual_base = unsafe { core::ptr::addr_of!(KERNEL_VIRTUAL_BASE).read_volatile() };
        match unsafe { e1000::initialize(function, hhdm, physical, virtual_base) } {
            Ok(mac) => {
                write("NIC Intel 82574L: filas DMA TX/RX ativas, MAC=");
                for byte in mac {
                    write_hex_byte(byte);
                }
                write("; envio/recepcao polling disponiveis.\r\n");
                match e1000::request_dhcp_lease() {
                    Ok(Some(lease)) => {
                        write("DHCP IPv4: concessao recebida ");
                        for (index, octet) in lease.address.iter().enumerate() {
                            if index != 0 {
                                write(".");
                            }
                            write_u32(*octet as u32);
                        }
                        write("\r\n");
                    }
                    Ok(None) => {
                        write("DHCP IPv4: servidor sem resposta; interface sem endereco.\r\n")
                    }
                    Err(error) => {
                        write("DHCP IPv4 indisponivel: ");
                        write(error.message());
                        write(".\r\n");
                    }
                }
            }
            Err(error) => {
                write("NIC Intel 82574L indisponivel: ");
                write(error.message());
                write(".\r\n");
            }
        }
    }
}

#[cfg(target_os = "none")]
fn report_intel_framebuffer_status() {
    if FRAMEBUFFER_ACTIVE.load(Ordering::Acquire) {
        write("; framebuffer ativo, pixels escritos pela CPU)");
    } else {
        write("; framebuffer inativo, sem acesso gráfico)");
    }
}

#[cfg(target_os = "none")]
fn write_hex_byte(value: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    serial_write(HEX[(value >> 4) as usize]);
    serial_write(HEX[(value & 0x0f) as usize]);
}

#[cfg(target_os = "none")]
fn write_hex_nibble(value: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    serial_write(HEX[(value & 0x0f) as usize]);
}

#[cfg(target_os = "none")]
fn write_hex_word(value: u16) {
    write_hex_byte((value >> 8) as u8);
    write_hex_byte(value as u8);
}

#[cfg(target_os = "none")]
unsafe fn out(port: u16, value: u8) {
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
    }
}

#[cfg(target_os = "none")]
unsafe fn input(port: u16) -> u8 {
    let value: u8;
    unsafe {
        asm!("in al, dx", in("dx") port, out("al") value, options(nomem, nostack, preserves_flags));
    }
    value
}

#[cfg(target_os = "none")]
struct PanicSerial;

#[cfg(target_os = "none")]
impl FmtWrite for PanicSerial {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for byte in text.bytes() {
            if !serial_write(byte) {
                return Err(fmt::Error);
            }
        }
        Ok(())
    }
}

#[panic_handler]
#[cfg(target_os = "none")]
fn panic(info: &PanicInfo) -> ! {
    record_error(KernelError::KernelPanic);
    write("\r\n=== CAPTURA DE PANIC ===\r\n");
    write("Causa: ");
    let _ = PanicSerial.write_fmt(format_args!("{}\r\n", info.message()));
    if let Some(location) = info.location() {
        write("Local: ");
        write(location.file());
        write(":");
        write_u32(location.line());
        write(":");
        write_u32(location.column());
        write("\r\n");
    } else {
        write("Local: indisponível\r\n");
    }
    write("Registradores do ponto de panic indisponíveis; faults de CPU usam a captura IDT.\r\n");
    write("Captura somente serial e volátil; reiniciar perde este registro.\r\n");
    write("Modo de recuperação serial ativo; digite Enter para consultar erros.\r\n");
    loop {
        match serial_read() {
            Ok(byte) if byte == b'\r' || byte == b'\n' => {
                report_errors();
                write("Modo de recuperação ativo; núcleo sem operações de dispositivo.\r\n");
            }
            Ok(_) => {}
            Err(_) => core::hint::spin_loop(),
        }
    }
}
