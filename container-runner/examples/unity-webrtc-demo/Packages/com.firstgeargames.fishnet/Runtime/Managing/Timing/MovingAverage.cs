using FishNet.Documenting;
using System;

namespace FishNet.Managing.Timing
{
    [APIExclude]
    public class MovingAverage : IDisposable
    {
        #region Public.
        /// <summary>
        /// Average from samples favoring the most recent sample.
        /// </summary>
        public float Average { get; private set; }
        /// <summary>
        /// Sample size being used.
        /// </summary>
        public int SampleSize { get; private set; }
        #endregion

        /// <summary>
        /// Next index to write a sample to.
        /// </summary>
        private int _writeIndex;
        /// <summary>
        /// Collected samples.
        /// </summary>
        private float[] _samples;
        /// <summary>
        /// Number of samples written. Will be at most samples size.
        /// </summary>
        private int _writtenSamples;
        /// <summary>
        /// Samples accumulated over queue.
        /// </summary>
        private float _sampleAccumulator;

        public MovingAverage(int sampleSize)
        {
            if (sampleSize < 2)
            {
                NetworkManagerExtensions.LogWarning("Using a sampleSize of less than 2 will always return the most recent value as Average.");
                sampleSize = 1;
            }

            SampleSize = sampleSize;
            _samples = new float[sampleSize];
        }

        /// <summary>
        /// Computes a new windowed average each time a new sample arrives
        /// </summary>
        /// <param name = "newSample"></param>
        public void ComputeAverage(float newSample)
        {
            if (_samples.Length <= 1)
            {
                Average = newSample;
                return;
            }

            _sampleAccumulator += newSample;
            _samples[_writeIndex] = newSample;

            // Increase writeIndex.
            _writeIndex++;
            _writtenSamples = Math.Max(_writtenSamples, _writeIndex);
            if (_writeIndex >= _samples.Length)
                _writeIndex = 0;

            Average = _sampleAccumulator / _writtenSamples;

            /* If samples are full then drop off
             * the oldest sample. This will always be
             * the one just after written. The entry isn't
             * actually removed from the array but will
             * be overwritten next sample. */
            if (_writtenSamples >= _samples.Length)
                _sampleAccumulator -= _samples[_writeIndex];
        }

        /// <summary>
        /// Resets values.
        /// </summary>
        public void Reset()
        {
            _sampleAccumulator = 0f;
            _writeIndex = 0;
            _writtenSamples = 0;
        }

        public void Dispose()
        {
            Reset();
        }
    }
}