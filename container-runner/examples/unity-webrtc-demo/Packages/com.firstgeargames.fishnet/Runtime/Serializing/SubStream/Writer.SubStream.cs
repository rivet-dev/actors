namespace FishNet.Serializing
{
    public partial class Writer
    {
        /// <summary>
        /// Writes a SubStream.
        /// </summary>
        /// <param name = "value">Substream</param>
        public void WriteSubStream(SubStream value)
        {
            // Uninitialized substream, write Length as -1
            if (!value.Initialized)
            {
                WriteInt32(SubStream.UNINITIALIZED_LENGTH);
            }
            else
            {
                PooledWriter bufferWriter = value.GetWriter();

                // Write length and data
                WriteInt32(bufferWriter.Length);
                WriteUInt8Array(bufferWriter.GetBuffer(), 0, bufferWriter.Length);
            }
        }
    }
}