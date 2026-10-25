// Italian strings (reference language). Every other language must provide the
// same keys: the `Messages` type makes a missing key a compile error.
export const it = {
  app: {
    name: "Konvert",
  },
  drop: {
    title: "Trascina qui un file",
    subtitle: "Audio o video: MP3, WAV, FLAC, MP4, MOV, MKV e molti altri",
    release: "Rilascia per aprire",
    choose: "Scegli file…",
    pickerTitle: "Scegli il file da convertire",
    pickerFilter: "File audio e video",
    privacy: "Tutto avviene sul tuo computer. Nessun file viene caricato online.",
  },
  analyzing: "Analisi del file…",
  file: {
    change: "Cambia file",
    audio: "Audio",
    video: "Video",
    videoWithAudio: "Video con audio",
    channels: {
      mono: "Mono",
      stereo: "Stereo",
      many: "{count} canali",
    },
    noAudio: "senza audio",
    coverArt: "copertina",
  },
  formats: {
    title: "Converti in",
    groupAudio: "Formato audio",
    groupVideo: "Formato video",
    groupAudioOnly: "Solo audio",
    lossless: "Senza perdita",
    sameFormat: "Stesso formato",
  },
  quality: {
    title: "Qualità",
    max: "Massima qualità",
    balanced: "Bilanciata",
    small: "File piccolo",
    maxHint: "La migliore qualità possibile per questo formato. File più grandi.",
    balancedHint: "Ottimo compromesso tra qualità e dimensione. Consigliata.",
    smallHint: "Dimensione ridotta, adatta a condividere. Qualità inferiore.",
  },
  summary: {
    estimate: "Dimensione stimata",
    approximate: "circa {size}",
    remux: "Senza ricodifica: veloce e senza perdita",
    remuxShort: "Senza ricodifica",
  },
  warnings: {
    lossyToLossless:
      "Il file originale è già compresso con perdita: convertirlo in un formato senza perdita non recupera la qualità persa e produce un file più grande.",
    lossyReencode:
      "Il file verrà ricompresso con perdita: la qualità può peggiorare leggermente rispetto all'originale.",
    losslessToLossy:
      "Conversione con perdita: il file sarà molto più piccolo, con una lieve riduzione della qualità.",
    gifLimitations:
      "Le GIF hanno al massimo 256 colori e producono file grandi: adatte a clip brevi.",
    audioDropped: "Il formato GIF non contiene audio: la traccia audio verrà scartata.",
    coverArtDropped: "Questo formato non può contenere la copertina: verrà omessa.",
    subtitlesDropped: "I sottotitoli non verranno conservati.",
    extraAudioTracksDropped: "Verrà conservata solo la prima traccia audio.",
    channelsDownmixed: "Questo formato supporta al massimo due canali: l'audio verrà convertito in stereo.",
  },
  actions: {
    convert: "Converti",
    cancel: "Annulla",
    cancelling: "Annullamento…",
    open: "Apri file",
    reveal: "Mostra nella cartella",
    another: "Converti un altro file",
    retry: "Riprova",
    back: "Indietro",
    chooseAnother: "Scegli un altro file",
  },
  progress: {
    queued: "In attesa…",
    preparing: "Preparazione…",
    converting: "Conversione in corso",
    remaining: "Circa {time} rimanenti",
    almostDone: "Quasi finito…",
    speed: "{speed}× più veloce del tempo reale",
    finalizing: "Finalizzazione…",
  },
  done: {
    title: "Conversione completata",
    saved: "Salvato come {name}",
  },
  cancelled: "Conversione annullata. Nessun file è stato creato.",
  errors: {
    title: "Qualcosa non ha funzionato",
    details: "Dettagli tecnici",
    engine_missing:
      "Il motore di conversione non è stato trovato. Reinstalla Konvert per ripristinarlo.",
    input_not_found: "Il file non esiste più o non è accessibile.",
    not_media: "Questo file non contiene audio o video.",
    corrupt_input: "Il file sembra danneggiato o in un formato non supportato.",
    unsupported_conversion: "Questa conversione non è supportata per questo file.",
    disk_full: "Spazio su disco insufficiente per salvare il file convertito.",
    permission_denied:
      "Accesso negato: Konvert non può leggere il file o scrivere nella cartella di destinazione.",
    cancelled: "Conversione annullata.",
    engine_failed: "La conversione non è riuscita.",
    io: "Errore durante la lettura o la scrittura del file.",
    unknown: "Si è verificato un errore imprevisto.",
  },
  time: {
    seconds: "{n} s",
    minutes: "{n} min",
    hoursMinutes: "{h} h {m} min",
    fewSeconds: "pochi secondi",
  },
} as const;

/** Same shape as `it`, with any string values. */
type DeepStrings<T> = { readonly [K in keyof T]: T[K] extends string ? string : DeepStrings<T[K]> };
export type Messages = DeepStrings<typeof it>;
