Both actually work

rodio::Decoder::new_mp3(IcyMetadataReader::new(
    reader,
    icy_headers.metadata_interval(),
    |_metadata| { /* Handle metadata updates if needed */ }
))

Mp3StreamDecoder::new(IcyMetadataReader::new(
    reader,
    icy_headers_clone.metadata_interval(),
    |_metadata| { /* Handle metadata updates if needed */ }
))
