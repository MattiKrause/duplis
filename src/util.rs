
#[macro_export]
macro_rules! dyn_clone_impl {
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

pub trait ChoiceInputReader {
    fn read_remaining(&mut self, buf: &mut String) -> std::io::Result<()>;
}

impl ChoiceInputReader for std::io::Stdin {
    fn read_remaining(&mut self, buf: &mut String) -> std::io::Result<()> {
        use std::io::BufRead;
        self.lock().read_line(buf).map(|_| ())
    }
}