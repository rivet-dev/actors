using System;
using FishNet.Managing;
using GameKit.Dependencies.Utilities;

namespace FishNet.Serializing
{
    /// <summary>
    /// Special reader/writer buffer struct that can be used in Fishnet RPCs or Broadcasts, as arguments or part of structs
    /// Use cases:
    /// - replacement for stream sort of
    /// - instead of always allocating some arrays T[] and sending that over RPCs/Broadcast, you can use SubStream
    /// - you can pass SubStream into objects via reference 'ref', and those objects write/read state, useful for dynamic length reconcile (items, inventory, buffs, etc...)
    /// - sending data inside OnServerSpawn to clients via TargetRPC
    /// - instead of writting custom serializers for big struct, you can use SubStream inside RPCs/Broadcasts
    /// Pros:
    /// - reading is zero copy, reads directly from FishNet buffers
    /// - everything is pooled
    /// - ease of use
    /// - SubStream can also be left uninitialized (default)
    /// - Can work safely with multiple receivers in Broadcasts, as long as you read data in the same order
    /// Cons:
    /// - no reading over length protection, you have to know how much data you are reading, due to buffer being red can be larger than substreams buffer
    /// - writing buffers are also pooled, but there is a copy (since you write into it, then what is written is copied into fishnet internal buffer, but it's byte copy (fast)
    /// - have to use Dispose() to return buffers to pool, or it may result in memory leak
    /// - reading in multiple receiver methods (for same client) in Broadcasts, you have extra deserialization processing per each method
    /// - might be unsafe to use this to send from clients (undefined data length), but so is sending T[] or List
    /// <T>
    /// from clients
    /// - not to be used for IReplicateData/input structs, because underlying reading buffer may be changed where as IReplicateData structs are stored internally in replay buffer (substream buffer is not)
    /// Note:
    /// - If you write/read custom structs ONLY via SubStream, automatic serializer will not pick those up. Mark those custom structs with [FishNet.CodeGenerating.IncludeSerialization].
    /// Codegen detects only custom structs that are used in RPC/Broadcast methods, not in SubStream.
    /// </summary>
    public struct SubStream : IResettable
    {
        /// <summary>
        /// Is Substream initialized (can be read from or written to)
        /// </summary>
        public bool Initialized { get; private set; }
        /// <summary>
        /// Returns Length of substream data
        /// </summary>
        public int Length
        {
            get
            {
                if (_writer != null)
                    return _writer.Length;
                if (_reader != null)
                    return _reader.Length;

                return UNINITIALIZED_LENGTH;
            }
        }
        /// <summary>
        /// Returns remaining bytes to read from substream
        /// </summary>
        public int Remaining => _reader != null ? _reader.Remaining : UNINITIALIZED_LENGTH;
        /// <summary>
        /// Returns NetworkManager that Substream was initialized with
        /// </summary>
        public NetworkManager NetworkManager
        {
            get
            {
                if (_writer != null)
                    return _writer.NetworkManager;
                if (_reader != null)
                    return _reader.NetworkManager;

                return null;
            }
        }
        private PooledReader _reader;
        private int _startPosition;
        private PooledWriter _writer;
        private bool _disposed;
        /// <summary>
        /// Length to use when SubStream is not initialized.
        /// </summary>
        public const int UNINITIALIZED_LENGTH = -1;

        /// <summary>
        /// Creates SubStream for writing, use this before sending into RPC or Broadcast
        /// </summary>
        /// <param name = "manager">Need to include network manager for handling of networked IDs</param>
        /// <param name = "minimumLength">Minimum expected length of data, that will be written</param>
        /// <returns>Returns writer of SubStream</returns>
        public static SubStream StartWriting(NetworkManager manager, out PooledWriter writer, int minimumLength = 0)
        {
            if (minimumLength == 0)
                writer = WriterPool.Retrieve(manager);
            else
                writer = WriterPool.Retrieve(manager, minimumLength);

            SubStream stream = new()
            {
                _writer = writer,
                Initialized = true
            };

            return stream;
        }

        /// <summary>
        /// Starts reading from substream via Reader class. Do not forget do Dispose() after reading
        /// </summary>
        /// <param name = "reader">Reader to read data from</param>
        /// <returns>Returns true, if SubStream is initialized else false</returns>
        public bool StartReading(out Reader reader)
        {
            if (Initialized)
            {
                // reset reader, in case we are reading in multiple broadcasts delegates/events
                _reader.Position = _startPosition;
                reader = _reader;
                return true;
            }
            reader = null;
            return false;
        }

        public static SubStream CreateFromReader(Reader originalReader, int subStreamLength)
        {
            if (subStreamLength < 0)
            {
                originalReader.NetworkManager.LogError("SubStream length cannot be less than 0");
                return default;
            }

            byte[] originalReaderBuffer = originalReader.GetBuffer();

            // inherits reading buffer directly from fishnet reader
            ArraySegment<byte> arraySegment = new(originalReaderBuffer, originalReader.Position, subStreamLength);

            PooledReader newReader = ReaderPool.Retrieve(arraySegment, originalReader.NetworkManager);

            // advance original reader by length of substream data
            originalReader.Skip(subStreamLength);

            return new()
            {
                _startPosition = newReader.Position,
                _reader = newReader,
                _writer = null,
                _disposed = false,
                Initialized = true
            };
        }

        /// <summary>
        /// Resets reader to start position, so you can read data again from start of substream.
        /// </summary>
        /// <exception cref = "ArgumentException"></exception>
        public void ResetReaderToStartPosition()
        {
            if (_reader != null)
                _reader.Position = _startPosition;
            else
                NetworkManager.LogError("SubStream was not initialized as reader!");
        }

        /// <summary>
        /// Used internally to get writer of SubStream
        /// </summary>
        /// <exception cref = "ArgumentException"></exception>
        internal PooledWriter GetWriter()
        {
            if (!Initialized)
                NetworkManager.LogError("SubStream was not initialized, it has to be initialized properly either localy or remotely!");
            else if (_writer == null)
                NetworkManager.LogError($"GetWriter() requires SubStream to be initialized as writer! You have to create SubStream with {nameof(StartWriting)}()!");

            return _writer;
        }

        internal PooledReader GetReader()
        {
            if (!Initialized)
                NetworkManager.LogError("SubStream was not initialized, it has to be initialized properly either localy or remotely!");
            if (_reader == null)
                NetworkManager.LogError($"GetReader() requires SubStream to be initialized as reader!");

            return _reader;
        }

        /// <summary>
        /// Returns uninitialized SubStream. Can send safely over network, but cannot be read from (StartReading will return false).
        /// You can also use 'var stream = default;' instead.
        /// </summary>
        /// <returns>Empty SubStream</returns>
        internal static SubStream GetUninitialized()
        {
            return new()
            {
                Initialized = false
            };
        }

        /// <summary>
        /// Do not forget to call this after:
        /// - you stopped writing to Substream AND already sent it via RPCs/Broadcasts
        /// - you stoped reading from it inside RPCs/Broadcast receive event
        /// - if you use it in Reconcile method, you have dispose SubStream inside Dispose() of IReconcileData struct
        /// </summary>
        public void ResetState()
        {
            if (!_disposed) // dispose reader only once
            {
                _disposed = true;

                if (_reader != null)
                {
                    _reader.Store();
                    _reader = null;
                }
            }

            if (_writer != null)
            {
                if (_writer.Length < WriterPool.LENGTH_BRACKET) // 1000 is LENGTH_BRACKET
                    _writer.Store();
                else
                    _writer.StoreLength();

                _writer = null;
            }
        }

        public void InitializeState() { }
    }
}