namespace FishNet.Component.Transforming
{
    public enum AdaptiveInterpolationType
    {
        /// <summary>
        /// Adaptive interpolation is disabled. An exact interpolation value is used.
        /// </summary>
        Off = 0,
        /// <summary>
        /// Visual disturbances caused by desynchronization are definite without predicting future states.
        /// </summary>
        ExtremelyLow = 1,
        /// <summary>
        /// Visual disturbances caused by desynchronization are likely without predicting future states.
        /// </summary>
        VeryLow = 2,
        /// <summary>
        /// Visual disturbances caused by desynchronization are still possible but less likely.
        /// </summary>
        Low = 3,
        /// <summary>
        /// Visual disturbances caused by desynchronization are likely without predicting a small amount of future states.
        /// </summary>
        Moderate = 4,
        /// <summary>
        /// Visual disturbances caused by desynchronization are very unlikely. Graphics are using a generous amount interpolation.
        /// </summary>
        High = 5,
        /// <summary>
        /// Visual disturbances caused by desynchronization are extremely unlikely. Graphics are using a generous amount interpolation.
        /// </summary>
        VeryHigh = 6
    }
}