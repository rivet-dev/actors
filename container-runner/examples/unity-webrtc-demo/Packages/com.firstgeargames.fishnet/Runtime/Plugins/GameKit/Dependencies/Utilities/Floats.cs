using System;
using UnityEngine;

namespace GameKit.Dependencies.Utilities
{
    public static class Floats
    {
        /// <summary>
        /// Used to randomize float values.
        /// </summary>
        private static System.Random _random = new();

        /// <summary>
        /// Sets a source float to value if equal to or greater than tolerance.
        /// </summary>
        /// <param name = "source">Float to check against tolerance.</param>
        /// <param name = "tolerance">Tolerance float must be equal to or greater than to change to value.</param>
        /// <param name = "value">Value source is set to when breaking tolerance.</param>
        public static float SetIfOverTolerance(this float source, float tolerance, float value)
        {
            if (source >= tolerance)
                source = value;

            return source;
        }

        /// <summary>
        /// Sets a source float to value if equal to or less than tolerance.
        /// </summary>
        /// <param name = "source">Float to check against tolerance.</param>
        /// <param name = "tolerance">Tolerance float must be equal to or less than to change to value.</param>
        /// <param name = "value">Value source is set to when breaking tolerance.</param>
        public static float SetIfUnderTolerance(this float source, float tolerance, float value)
        {
            if (source <= tolerance)
                source = value;

            return source;
        }

        /// <summary>
        /// Returns how much time is left on an endTime. Returns -1 if no time is left.
        /// </summary>
        /// <returns></returns>
        public static float TimeRemainingValue(this float endTime)
        {
            float remaining = endTime - Time.time;
            // None remaining.
            if (remaining < 0f)
                return -1f;

            return endTime - Time.time;
        }

        /// <summary>
        /// Returns how much time is left on an endTime. Returns -1 if no time is left.
        /// </summary>
        /// <returns></returns>
        public static int TimeRemainingValue(this float endTime, bool useFloor = true)
        {
            float remaining = endTime - Time.time;
            // None remaining.
            if (remaining < 0f)
                return -1;

            float result = endTime - Time.time;
            return useFloor ? Mathf.FloorToInt(result) : Mathf.CeilToInt(result);
        }

        /// <summary>
        /// Returns time remaining as a string using hh:mm:ss.
        /// </summary>
        /// <param name = "value"></param>
        /// <param name = "segments">Number of places to return. 1 is seconds, 2 is minutes, 3 is hours. If a placement does not exist it is replaced with 00.</param>
        /// <param name = "emptyOnZero">True to return an empty string when value is 0 or less.</param>
        /// <returns></returns>
        public static string TimeRemainingText(this float value, byte segments, bool emptyOnZero = false)
        {
            if (emptyOnZero && value <= 0f)
                return string.Empty;

            int timeRounded = Math.Max(Mathf.RoundToInt(value), 0);
            TimeSpan t = TimeSpan.FromSeconds(timeRounded);

            int hours = Mathf.FloorToInt(t.Hours);
            int minutes = Mathf.FloorToInt(t.Minutes);
            int seconds = Mathf.FloorToInt(t.Seconds);

            string timeText;
            if (segments == 1)
            {
                seconds += minutes * 60;
                seconds += hours * 3600;
                timeText = string.Format("{0:D2}", seconds);
            }
            else if (segments == 2)
            {
                minutes += hours * 60;
                timeText = string.Format("{0:D2}:{1:D2}", minutes, seconds);
            }
            else
            {
                timeText = string.Format("{0:D2}:{1:D2}:{2:D2}", hours, minutes, seconds);
            }

            return timeText;
        }

        /// <summary>
        /// Provides a random inclusive int within a given range. Preferred over Unity's Random to eliminate confusion as Unity uses inclusive for floats max, and exclusive for int max.
        /// </summary>
        /// <param name = "minimum">Inclusive minimum value.</param>
        /// <param name = "maximum">Inclusive maximum value.</param>
        /// <returns></returns>
        public static float RandomInclusiveRange(float minimum, float maximum)
        {
            double min = Convert.ToDouble(minimum);
            double max = Convert.ToDouble(maximum);

            double result = _random.NextDouble() * (max - min) + min;
            return Convert.ToSingle(result);
        }

        /// <summary>
        /// Returns a random float between 0f and 1f.
        /// </summary>
        /// <returns></returns>
        public static float Random01()
        {
            return RandomInclusiveRange(0f, 1f);
        }

        /// <summary>
        /// Returns if a target float is within variance of the source float.
        /// </summary>
        /// <param name = "a"></param>
        /// <param name = "b"></param>
        /// <param name = "tolerance"></param>
        public static bool Near(this float a, float b, float tolerance = 0.01f)
        {
            return Mathf.Abs(a - b) <= tolerance;
        }

        /// <summary>
        /// Clamps a float and returns if the float required clamping.
        /// </summary>
        /// <param name = "value"></param>
        /// <param name = "min"></param>
        /// <param name = "max"></param>
        /// <param name = "clamped"></param>
        /// <returns></returns>
        public static float Clamp(float value, float min, float max, ref bool clamped)
        {
            clamped = value < min;
            if (clamped)
                return min;

            clamped = value > min;
            if (clamped)
                return max;

            clamped = false;
            return value;
        }

        /// <summary>
        /// Returns a float after being adjusted by the specified variance.
        /// </summary>
        /// <param name = "source"></param>
        /// <param name = "variance"></param>
        /// <returns></returns>
        public static float Variance(this float source, float variance)
        {
            float pickedVariance = RandomInclusiveRange(1f - variance, 1f + variance);
            return source * pickedVariance;
        }

        /// <summary>
        /// Sets a float value to result after being adjusted by the specified variance.
        /// </summary>
        /// <param name = "source"></param>
        /// <param name = "variance"></param>
        /// <returns></returns>
        public static void Variance(this float source, float variance, ref float result)
        {
            float pickedVariance = RandomInclusiveRange(1f - variance, 1f + variance);
            result = source * pickedVariance;
        }

        /// <summary>
        /// Returns negative-one, zero, or postive-one of a value instead of just negative-one or positive-one.
        /// </summary>
        /// <param name = "value">Value to sign.</param>
        /// <returns>Precise sign.</returns>
        public static float PreciseSign(float value)
        {
            if (value == 0f)
                return 0f;
            else
                return Mathf.Sign(value);
        }

        /// <summary>
        /// Returns if a float is within a range.
        /// </summary>
        /// <param name = "source">Value of float.</param>
        /// <param name = "rangeMin">Minimum of range.</param>
        /// <param name = "rangeMax">Maximum of range.</param>
        /// <returns></returns>
        public static bool InRange(this float source, float rangeMin, float rangeMax)
        {
            return source >= rangeMin && source <= rangeMax;
        }

        /// <summary>
        /// Randomly flips a float value.
        /// </summary>
        /// <param name = "value"></param>
        /// <returns></returns>
        public static float RandomlyFlip(this float value)
        {
            if (Ints.RandomInclusiveRange(0, 1) == 0)
                return value;
            else
                return value *= -1f;
        }
    }
}