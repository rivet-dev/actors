using System;

namespace FishNet.Utility
{

    /// <summary>
    /// Finds the next prime number on or after a value.
    /// </summary>
    public static class PrimeNumberFinder
    {
        public static uint GetNextPrime(uint number)
        {
            uint candidate = number < 2 ? 2 : number;

            while (!IsPrime(candidate))
                candidate++;

            return candidate;
        }
        
        /// <summary>
        /// Returns if a number is prime.
        /// </summary>
        private static bool IsPrime(uint number)
        {
            if (number < 2)
                return false;
            if (number == 2)
                return true;
            if (number % 2 == 0)
                return false;

            int limit = (int)Math.Sqrt(number);

            for (int i = 3; i <= limit; i += 2)
            {
                if (number % i == 0)
                    return false;
            }

            return true;
        }
    }

}