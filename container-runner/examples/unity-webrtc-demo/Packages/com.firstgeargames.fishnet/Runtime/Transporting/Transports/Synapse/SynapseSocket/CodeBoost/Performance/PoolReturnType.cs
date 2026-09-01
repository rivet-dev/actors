namespace CodeBoost.Performance
{

    /// <summary>
    /// How to handle values when they may need to be pooled.
    /// </summary>
    /// <remarks>This can be used to allow pooling of collection values while optionally pooling the collection itself.</remarks>
    public enum PoolReturnType 
    {
        /// <summary>
        /// Do nothing with the object.
        /// </summary>
        None = 0,
        /// <summary>
        /// Return the object.
        /// </summary>
        Return = 1,
    }
}
