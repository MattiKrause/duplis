

macro_rules! dyn_clone {
    ($dcname: ident, $tname: path) => {
        pub trait $dcname {
            fn dyn_clone(&self) -> Box<dyn $tname>;
        }

        impl <T: 'static + $tname + Clone> $dcname for T {
            fn dyn_clone(&self) -> Box<dyn $tname> {
                Box::new(self.clone())
            }
        }
    };
}

dyn_clone!(DynCloneSetOrder, crate::set_order::SetOrder);