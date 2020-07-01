use super::super::StrPtr;
use super::heap_values::*;
use super::{Heap, ALIGNMENT};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

/// A managed pointer to a `ObjectDataPtr`. this level of indirection is
/// needed to update objects when reallocated
///
/// This looks like this:
/// Tag(1) | pointer to ObjectDataPtr(1)
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct ObjectPtr<'a> {
    ptr: *mut Tag,
    _phantom: PhantomData<&'a ()>,
}

/// A managed pointer to an Object, specified by a Class
///
/// It looks like this:
/// Tag(1) | field(1) | field(1) | ...
#[derive(Debug, PartialEq, Clone, Copy)]
#[repr(transparent)]
pub struct ObjectDataPtr<'a> {
    ptr: *mut Tag,
    _phantom: PhantomData<&'a ()>,
}

impl<'a> HeapPtr for ObjectDataPtr<'a> {
    fn get_ptr(&self) -> *mut Tag {
        return self.ptr;
    }

    fn get_data_size(&self, heap: &Heap) -> usize {
        let tag = unsafe { *self.ptr };
        let class_tag = tag.class_tag;
        let num_elements = heap.get_class_size(class_tag);
        return num_elements * ALIGNMENT;
    }
}

impl<'a> ObjectDataPtr<'a> {
    /// This function is unsafe, because (1) we do not check that the class_tag
    /// is valid, and (2) we assume that `ptr` is valid.
    pub unsafe fn new(ptr: *mut Tag) -> Self {
        assert_eq!((*ptr).type_tag, TypeTag::Class);
        ObjectDataPtr {
            ptr,
            _phantom: PhantomData,
        }
    }

    pub fn class_tag(&self) -> u16 {
        let tag = unsafe { *self.ptr };
        assert_eq!(tag.type_tag, TypeTag::Class);
        tag.class_tag
    }

    pub fn read_at(&self, heap: &'a Heap, index: usize) -> Option<AnyPtr<'a>> {
        let type_tag = self.class_tag();
        let len = heap.get_class_size(type_tag);
        assert!(index < len);
        let values = unsafe { self.ptr.add(1) as *mut *mut Tag };
        let ptr = unsafe { *values.add(index) };

        if ptr.is_null() {
            None
        } else {
            Some(unsafe { AnyPtr::new(ptr) })
        }
    }

    pub fn write_at<P: HeapPtr>(&self, heap: &'a Heap, index: usize, value: P) {
        let type_tag = self.class_tag();
        let len = heap.get_class_size(type_tag);
        assert!(index < len);
        let values = unsafe { self.ptr.add(1) as *mut *mut Tag };
        let ptr = unsafe { values.add(index) };
        unsafe {
            ptr.write(value.get_ptr());
        }
    }

    /// if name is found, write to it. if not, transition, clone, write, and
    /// return new pointer. this should be called by ObjectPtr only
    #[must_use]
    fn insert<P: HeapPtr + Copy>(self, heap: &'a Heap, name: StrPtr, value: P) -> Option<Self> {
        let class_tag = self.class_tag();
        let mut classes = heap.classes.borrow_mut();
        let class = classes.get_class(class_tag);
        match class.lookup(name) {
            Some(offset) => {
                drop(class);
                drop(classes);
                self.write_at(heap, offset, value);
                Some(self)
            }
            None => {
                let size = class.size;
                drop(class);
                let new_tag = classes.transition(class_tag, name);
                drop(classes);
                let new_object = heap.alloc_object_data(new_tag)?;
                for i in 0..size {
                    if let Some(val) = self.read_at(heap, i) {
                        new_object.write_at(heap, i, val);
                    }
                }
                new_object.write_at(heap, size, value);
                Some(new_object)
            }
        }
    }

    pub fn get(&self, heap: &'a Heap, name: StrPtr) -> Option<AnyPtr<'a>> {
        let class_tag = self.class_tag();
        let classes = heap.classes.borrow();
        let class = classes.get_class(class_tag);
        let offset = class.lookup(name)?;
        self.read_at(heap, offset)
    }
}

impl<'a> ObjectPtr<'a> {
    pub unsafe fn new(ptr: *mut Tag) -> Self {
        Self {
            ptr,
            _phantom: PhantomData,
        }
    }
    /// if name is found, write to it. if not, transition, clone, write, and
    /// update pointer
    /// TODO: updating this pointer in particular isn't enough. i think we
    /// have to have a double-pointer situation
    /// (ObjectPtr -> ArrayPtr -> [u8; n])
    pub fn insert<P: HeapPtr + Copy>(
        &mut self,
        heap: &'a Heap,
        name: StrPtr,
        value: P,
    ) -> Option<P> {
        let data = &mut **self;
        let new = data.insert(heap, name, value)?;
        unsafe { *(self.ptr.add(1) as *mut ObjectDataPtr) = new };
        Some(value)
    }
}
impl<'a> Deref for ObjectPtr<'a> {
    type Target = ObjectDataPtr<'a>;
    fn deref(&self) -> &Self::Target {
        unsafe { &*(self.ptr.add(1) as *const ObjectDataPtr) }
    }
}
impl<'a> DerefMut for ObjectPtr<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *(self.ptr.add(1) as *mut ObjectDataPtr) }
    }
}
impl<'a> HeapPtr for ObjectPtr<'a> {
    fn get_ptr(&self) -> *mut Tag {
        self.ptr
    }
    fn get_data_size(&self, _heap: &Heap) -> usize {
        4
    }
}
