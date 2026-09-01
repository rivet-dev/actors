using CodeBoost.CodeAnalysis;

namespace CodeBoost.Performance
{

    /// <summary>
    /// Implement to reset values when returning to a pool, as well to initialize when renting from a pool.
    /// </summary>
    public interface IPoolResettable
    {
        /// <summary>
        /// Resets values when being placed into a pool.
        /// </summary>
        [CreateSignature]
        void OnReturn();

        /// <summary>
        /// Initializes values when being retrieved from a pool.
        /// </summary>
        [CreateSignature]
        void OnRent();
    }
}
