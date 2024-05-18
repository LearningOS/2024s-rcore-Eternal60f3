use super::{
    block_cache_sync_all, get_block_cache, BlockDevice, DirEntry, DiskInode, DiskInodeType,
    EasyFileSystem, DIRENT_SZ,
};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::{Mutex, MutexGuard};
/// Virtual filesystem layer over easy-fs
pub struct Inode {
    block_id: usize,
    block_offset: usize,
    fs: Arc<Mutex<EasyFileSystem>>,
    block_device: Arc<dyn BlockDevice>,
}

// 所有 pub 的函数因为是对外开放的，所以都需要申请文件系统的锁
// 而所有私有函数都不需要申请，同时为了避免死锁，私有函数不能使用 pub 函数
impl Inode {
    /// Create a vfs inode
    pub fn new(
        block_id: u32,
        block_offset: usize,
        fs: Arc<Mutex<EasyFileSystem>>,
        block_device: Arc<dyn BlockDevice>,
    ) -> Self {
        Self {
            block_id: block_id as usize,
            block_offset,
            fs,
            block_device,
        }
    }
    /// get inode_id private
    fn id(&self) -> u32 {
        self.read_disk_inode(|disk_inode| disk_inode.id)
    }
    /// is dir?
    fn is_dir_pri(&self) -> bool {
        self.read_disk_inode(|disk_inode| disk_inode.is_dir())
    }
    /// is file?
    #[allow(unused)]
    fn is_file(&self) -> bool {
        self.read_disk_inode(|disk_inode| disk_inode.is_file())
    }
    /// get size
    fn size(&self) -> u32 {
        self.read_disk_inode(|disk_inode| disk_inode.size as u32)
    }
    fn increase_link(&self) {
        self.modify_disk_inode(|disk_inode| {
            disk_inode.link_cnt += 1;
        });
    }
    fn decrease_link(&self, fs: &mut MutexGuard<EasyFileSystem>) {
        self.modify_disk_inode(|disk_inode| {
            disk_inode.link_cnt -= 1;
            
            if disk_inode.link_cnt == 0 {
                let size = disk_inode.size;
                let data_blocks_dealloc = disk_inode.clear_size(&self.block_device);
                assert!(data_blocks_dealloc.len() == DiskInode::total_blocks(size) as usize);
                for data_block in data_blocks_dealloc.into_iter() {
                    fs.dealloc_data(data_block);
                }
            }
        });
        
        // 不能和上面嵌套，否则会造成block_cache的死锁
        // 这里没有立即写回，因为按照目前的理解，没必要，
        //  但是其他凡是对block_cache进行了修改的都运行了block_cache_sync_all函数
        //  来进行立即写回
        if self.link_cnt() == 0 {
            fs.dealloc_disk_inode(self.id());
        }
    }
    #[allow(unused)]
    fn link_cnt(&self) -> usize {
        self.read_disk_inode(|disk_inode| {
            disk_inode.link_cnt as usize
        })
    }
    /// Call a function over a disk inode to read it
    fn read_disk_inode<V>(&self, f: impl FnOnce(&DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .read(self.block_offset, f)
    }
    /// Call a function over a disk inode to modify it
    fn modify_disk_inode<V>(&self, f: impl FnOnce(&mut DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .modify(self.block_offset, f)
    }
    /// Find inode under a disk inode by name
    fn find_inode_id(&self, name: &str, disk_inode: &DiskInode) -> Option<u32> {
        // assert it is a directory
        assert!(disk_inode.is_dir());
        let file_count = (disk_inode.size as usize) / DIRENT_SZ;
        let mut dirent = DirEntry::empty();
        for i in 0..file_count {
            assert_eq!(
                disk_inode.read_at(DIRENT_SZ * i, dirent.as_bytes_mut(), &self.block_device,),
                DIRENT_SZ,
            );
            if dirent.name() == name {
                return Some(dirent.inode_id() as u32);
            }
        }
        None
    }
    /// Find inode under current inode by name
    pub fn find(&self, name: &str) -> Option<Arc<Inode>> {
        let fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            self.find_inode_id(name, disk_inode).map(|inode_id| {
                let (block_id, block_offset) = fs.get_disk_inode_pos(inode_id);
                Arc::new(Self::new(
                    block_id,
                    block_offset,
                    self.fs.clone(),
                    self.block_device.clone(),
                ))
            })
        })
    }
    /// Increase the size of a disk inode
    fn increase_size(
        &self,
        new_size: u32,
        disk_inode: &mut DiskInode,
        fs: &mut MutexGuard<EasyFileSystem>,
    ) {
        if new_size < disk_inode.size {
            return;
        }
        let blocks_needed = disk_inode.blocks_num_needed(new_size);
        let mut v: Vec<u32> = Vec::new();
        for _ in 0..blocks_needed {
            v.push(fs.alloc_data());
        }
        disk_inode.increase_size(new_size, v, &self.block_device);
    }
    /// make a dirent in dir
    fn append_dirent(&self, name: &str, inode_id: i32, fs: &mut MutexGuard<EasyFileSystem>) {
        assert!(self.is_dir_pri(), "Don't use append_dirent in file");
        let exist_count = (self.size() as usize) / DIRENT_SZ;
        let mut dirent = DirEntry::empty();
        let new_dirent = DirEntry::new(name, inode_id);
        let mut flag = false;
        for i in 0..exist_count {
            let offset = i * DIRENT_SZ;
            self.read_disk_inode(|disk_inode| {
                assert_eq!(disk_inode.read_at(offset, dirent.as_bytes_mut(), &self.block_device), DIRENT_SZ);
            });
            if !dirent.is_valid() {
                self.modify_disk_inode(|disk_inode| {
                    assert_eq!(disk_inode.write_at(offset, new_dirent.as_bytes(), &self.block_device), DIRENT_SZ);
                });
                flag = true;
                break;
            }
        }
        
        if !flag {
            let new_size = (exist_count + 1) * DIRENT_SZ;
            self.modify_disk_inode(|disk_inode| {
                self.increase_size(new_size as u32, disk_inode, fs);
                let offset = exist_count * DIRENT_SZ;
                assert_eq!(disk_inode.write_at(offset, new_dirent.as_bytes(), &self.block_device), DIRENT_SZ);
            });
        }
        
        let inode = {
            let (block_id, block_offset) = fs.get_disk_inode_pos(inode_id as u32);
            Arc::new(Self::new(
                block_id,
                block_offset,
                self.fs.clone(),
                self.block_device.clone(),
            ))
        };
        inode.increase_link();
    }
    /// remove a dirent in dir
    fn remove_dirent(&self, dirent_idx: usize, fs: &mut MutexGuard<EasyFileSystem>) {
        assert!(self.is_dir_pri());

        let mut dirent = DirEntry::empty();
        self.read_disk_inode(|disk_inode| {
            disk_inode.read_at(dirent_idx * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device);
        });

        self.modify_disk_inode(|disk_inode| {
            let illegal_dirent = DirEntry::new("has been removed", -1);
            disk_inode.write_at(dirent_idx * DIRENT_SZ, illegal_dirent.as_bytes(), &self.block_device);
        });

        let inode_id = dirent.inode_id();
        let inode = {
            let (block_id, block_offset) = fs.get_disk_inode_pos(inode_id as u32);
            Arc::new(Self::new(
                block_id,
                block_offset,
                self.fs.clone(),
                self.block_device.clone(),
            ))
        };
        inode.decrease_link(fs);
    }
    /// Create inode under current inode by name
    pub fn create(&self, name: &str) -> Option<Arc<Inode>> {
        let mut fs = self.fs.lock();
        let op = |root_inode: &DiskInode| {
            // assert it is a directory
            assert!(root_inode.is_dir());
            // has the file been created?
            self.find_inode_id(name, root_inode)
        };
        if self.read_disk_inode(op).is_some() {
            return None;
        }
        // create a new file
        // alloc a inode with an indirect block
        let new_inode_id = fs.alloc_inode();
        // initialize inode
        let (new_inode_block_id, new_inode_block_offset) = fs.get_disk_inode_pos(new_inode_id);
        get_block_cache(new_inode_block_id as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(new_inode_block_offset, |new_inode: &mut DiskInode| {
                new_inode.initialize(new_inode_id, DiskInodeType::File);
            });
        self.append_dirent(name, new_inode_id as i32, &mut fs);

        let (block_id, block_offset) = fs.get_disk_inode_pos(new_inode_id);
        block_cache_sync_all();
        // return inode
        Some(Arc::new(Self::new(
            block_id,
            block_offset,
            self.fs.clone(),
            self.block_device.clone(),
        )))
        // release efs lock automatically by compiler
    }
    /// List inodes under current inode
    pub fn ls(&self) -> Vec<String> {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;
            let mut v: Vec<String> = Vec::new();
            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                assert_eq!(
                    disk_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device,),
                    DIRENT_SZ,
                );
                v.push(String::from(dirent.name()));
            }
            v
        })
    }
    /// Read data from current inode
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| disk_inode.read_at(offset, buf, &self.block_device))
    }
    /// Write data to current inode
    pub fn write_at(&self, offset: usize, buf: &[u8]) -> usize {
        let mut fs = self.fs.lock();
        let size = self.modify_disk_inode(|disk_inode| {
            self.increase_size((offset + buf.len()) as u32, disk_inode, &mut fs);
            disk_inode.write_at(offset, buf, &self.block_device)
        });
        block_cache_sync_all();
        size
    }
    /// Clear the data in current inode
    pub fn clear(&self) {
        let mut fs = self.fs.lock();
        self.modify_disk_inode(|disk_inode| {
            let size = disk_inode.size;
            let data_blocks_dealloc = disk_inode.clear_size(&self.block_device);
            assert!(data_blocks_dealloc.len() == DiskInode::total_blocks(size) as usize);
            for data_block in data_blocks_dealloc.into_iter() {
                fs.dealloc_data(data_block);
            }
        });
        block_cache_sync_all();
    }
    /// 将 new_name 也链接到 old_name 对应的 inode
    pub fn linkat(&self, old_name: &str, new_name: &str) -> isize {
        assert!(self.is_dir_pri(), "Don't use link in file");
        let mut fs = self.fs.lock();
        if old_name == new_name {
            return -1;
        }
        let old_inode_id = self.read_disk_inode(|disk_inode| {
            if let Some(old_inode_id) = self.find_inode_id(old_name, disk_inode) {
                old_inode_id as i32
            } else {
                -1
            }
        });
        if old_inode_id != -1 {
            // 不能放到上面嵌套，否则会导致block_cache陷入死锁
            self.append_dirent(new_name, old_inode_id as i32, &mut fs);
            0
        } else {
            -1
        }
    }
    /// 取消 name 与文件的链接
    pub fn unlinkat(&self, name: &str) -> isize {
        assert!(self.is_dir_pri());
        let mut fs = self.fs.lock();
        let exist_count = self.size() as usize / DIRENT_SZ;
        let mut dirent = DirEntry::empty();
        let dirent_id = self.read_disk_inode(|disk_inode| {
            for i in 0..exist_count {
                let offset = i * DIRENT_SZ;
                assert_eq!(disk_inode.read_at(offset, dirent.as_bytes_mut(), &self.block_device), DIRENT_SZ);
                if dirent.is_valid() && dirent.name() == name {
                    return i as isize;
                }
            }   
            -1
        });
        if dirent_id != -1 {
            // 不能放到上面嵌套，否则会导致block_cache陷入死锁
            self.remove_dirent(dirent_id as usize, &mut fs);
            0
        } else {
            -1
        }
    }
    /// get inode_id public
    pub fn get_id(&self) -> u32 {
        let _fs = self.fs.lock();
        self.id()
    }
    /// is dir? public
    pub fn is_dir(&self) -> bool {
        let _fs = self.fs.lock();
        self.is_dir_pri()
    }
    /// get inode_id public 
    pub fn nlink(&self) -> u32 {
        let _fs = self.fs.lock();
        self.link_cnt() as u32
    }
}