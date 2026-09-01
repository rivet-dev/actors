namespace FishNet.Managing.Timing
{
    /// <summary>
    /// How ticks are rounded when using time.
    /// </summary>
    public enum TickRounding
    {
        /// <summary>
        /// Rounds up.
        /// </summary>
        RoundUp,
        /// <summary>
        /// Rounds down.
        /// </summary>
        RoundDown,
        /// <summary>
        /// Rounds to the nearest whole.
        /// </summary>
        RoundNearest
    }
}