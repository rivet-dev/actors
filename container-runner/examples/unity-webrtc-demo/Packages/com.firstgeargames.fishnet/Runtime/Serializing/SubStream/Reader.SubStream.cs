namespace FishNet.Serializing
{
    public partial class Reader
    {
        /// <summary>
        /// Reads a substream. Start reading from it with StartReading method.
        /// </summary>
        /// <returns>Returns SubStream</returns>
        public SubStream ReadSubStream()
        {
            // read length of subStream
            int streamLength = ReadInt32();

            // if length is -1, it is invalid
            if (streamLength == SubStream.UNINITIALIZED_LENGTH)
            {
                // returns Uninitialized SubStream
                return SubStream.GetUninitialized();
            }

            return SubStream.CreateFromReader(this, streamLength);
        }
    }
}