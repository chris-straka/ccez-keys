package uk.ccez.whisper;

/**
 * Whisper on this phone, for the owner's other apps (bind to
 * uk.ccez.whisper/.WhisperService with action uk.ccez.whisper.HEAR).
 */
interface IWhisper {
    /** 0: ready. 1: no model yet (open ccez-whisper to get it). 2: getting it. */
    int state();

    /**
     * What's said in [sound]: 16 kHz mono 16-bit, raw or a WAV. [lang] is
     * an ISO code ("fr"), or "" for Whisper's guess. JSON
     * {"lang":"fr","segments":[{"t0":ms,"t1":ms,"text":"…"}]}, or null when
     * it couldn't. Takes about half the sound's length: call it off the
     * main thread. One at a time; others wait their turn.
     */
    String hear(in ParcelFileDescriptor sound, String lang);
}
