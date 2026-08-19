namespace FishNet.Object.Prediction
{
    public interface IReplicateData
    {
        /// <summary>
        /// Local tick when the data was created.
        /// </summary>
        /// <returns></returns>
        uint GetTick();

        /// <summary>
        /// Sets the local tick when data was created.
        /// </summary>
        /// <param name = "value"></param>
        void SetTick(uint value);

        /// <summary>
        /// Allows for any cleanup when the data is being discarded.
        /// </summary>
        void Dispose();
    }

    public interface IReconcileData
    {
        /// <summary>
        /// Local tick when the data was created.
        /// </summary>
        /// <returns></returns>
        uint GetTick();

        /// <summary>
        /// Sets the local tick when data was created.
        /// </summary>
        /// <param name = "value"></param>
        void SetTick(uint value);

        /// <summary>
        /// Allows for any cleanup when the data is being discarded.
        /// </summary>
        void Dispose();
    }
}