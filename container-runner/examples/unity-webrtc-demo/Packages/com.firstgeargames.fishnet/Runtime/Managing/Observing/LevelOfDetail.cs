namespace FishNet.Managing.Observing
{

    /// <summary>
    /// A configuration which affects the level of detail for a connection.
    /// </summary>
    public struct LevelOfDetail
    {
        /// <summary>
        /// How often data will send when on this level of detail.
        /// </summary>
        public ushort SendInterval;

        /// <summary>
        /// Distance a connection's objects must be within to use this LevelOfDetail.
        /// </summary>
        public ushort Distance;
    }
}