using System;

namespace GameKit.Dependencies.Utilities
{
    public static class Enums
    {
        public const int SHIFT_EVERYTHING_INT = ~0;
        public const uint SHIFT_EVERYTHING_UINT = ~0u;

        // 65535
        /// <summary>
        /// Determine an enum value from a given string. This can be an expensive function.
        /// </summary>
        /// <typeparam name = "T"></typeparam>
        /// <param name = "text">Text of string.</param>
        /// <param name = "defaultValue">Default value if enum couldn't be found.</param>
        /// <returns>Enum found or default value if no enum is found.</returns>
        public static T FromString<T>(string text, T defaultValue)
        {
            // If string is empty or null return default value.
            if (string.IsNullOrEmpty(text))
                return defaultValue;
            // If enum isn't defined return default value.
            if (!Enum.IsDefined(typeof(T), (string)text))
                return defaultValue;
            //Return parsed value.
            return (T)Enum.Parse(typeof(T), text, true);
        }

        /// <summary>
        /// Returns if whole(extended enum) has any of the part values.
        /// </summary>
        /// <param name = "whole"></param>
        /// <param name = "part">Values to check for within whole.</param>
        /// <returns>Returns true part is within whole.</returns>
        public static bool ContainsAllocated(this Enum whole, Enum part)
        {
            //If not the same type of Enum return false.
            /* Commented out for performance. Designer
             * should know better than to compare two different
             * enums. */
            //if (!SameType(value, target))
            //    return false;

            /* Convert enum values to ulong. With so few
             * values a uint would be safe, but should
             * the options expand ulong is safer. */
            ulong wholeNum = Convert.ToUInt64(whole);
            ulong partNum = Convert.ToUInt64(part);

            return (wholeNum & partNum) != 0;
        }

        /// <summary>
        /// Returns if part values contains any of whole(extended enum).
        /// </summary>
        /// <param name = "whole"></param>
        /// <param name = "part"></param>
        /// <returns>Returns true whole is within part.</returns>
        public static bool ReverseContains(this Enum whole, Enum part)
        {
            //If not the same type of Enum return false.
            /* Commented out for performance. Designer
             * should know better than to compare two different
             * enums. */
            //if (!SameType(value, target))
            //    return false;

            /* Convert enum values to ulong. With so few
             * values a uint would be safe, but should
             * the options expand ulong is safer. */
            ulong wholeNum = Convert.ToUInt64(whole);
            ulong partNum = Convert.ToUInt64(part);

            return (partNum & wholeNum) != 0;
        }

        /// <summary>
        /// Returns if an enum equals a specified value.
        /// </summary>
        /// <param name = "value"></param>
        /// <param name = "target"></param>
        /// <returns></returns>
        public static bool Equals(this Enum value, Enum target)
        {
            //If not the same type of Enum return false.
            /* Commented out for performance. Designer
             * should know better than to compare two different
             * enums. */
            //if (!SameType(value, target))
            //    return false;

            ulong valueNum = Convert.ToUInt64(value);
            ulong wholeNum = Convert.ToUInt64(target);

            return valueNum == wholeNum;
        }

        /// <summary>
        /// Returns if a is the same Enum as b.
        /// </summary>
        /// <param name = "a"></param>
        /// <param name = "target"></param>
        /// <returns></returns>
        public static bool SameType(Enum a, Enum b)
        {
            return a.GetType() == b.GetType();
        }

        /// <summary>
        /// Returns the highest numeric value for T.
        /// </summary>
        public static int GetHighestValue<T>()
        {
            Type enumType = typeof(T);
            /* Brute force enum values.
             * Linq Last/Max lookup throws for IL2CPP. */
            int highestValue = 0;
            Array pidValues = Enum.GetValues(enumType);
            foreach (T pid in pidValues)
            {
                object obj = Enum.Parse(enumType, pid.ToString());
                int value = Convert.ToInt32(obj);
                highestValue = Math.Max(highestValue, value);
            }

            return highestValue;
        }
    }
}