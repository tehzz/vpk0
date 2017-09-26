error_chain!{
    foreign_links {
        StrFromUtf8(::std::str::Utf8Error);
        Io(::std::io::Error);
    }
    errors {
        InvalidHeader {
            description("invalid header for vpk0 file"),
            display("invalid header for vpk0 file"),
        }
    }
}
