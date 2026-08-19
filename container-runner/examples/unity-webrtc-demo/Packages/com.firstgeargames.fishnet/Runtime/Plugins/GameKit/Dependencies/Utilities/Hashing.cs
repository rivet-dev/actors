namespace GameKit.Dependencies.Utilities
{
    public static class Hashing
    {
        private const uint FNV_offset_basis32 = 2166136261;
        private const uint FNV_prime32 = 16777619;
        private const ulong FNV_offset_basis64 = 14695981039346656037;
        private const ulong FNV_prime64 = 1099511628211;

        /// <summary>
        /// non cryptographic stable hash code,
        /// it will always return the same hash for the same
        /// string.
        /// This is simply an implementation of FNV-1 32 bit xor folded to 16 bit
        /// https://en.wikipedia.org/wiki/Fowler%E2%80%93Noll%E2%80%93Vo_hash_function
        /// </summary>
        /// <returns>The stable hash32.</returns>
        /// <param name = "txt">Text.</param>
        public static ushort GetStableHashU16(this string txt)
        {
            uint hash32 = txt.GetStableHashU32();

            return (ushort)((hash32 >> 16) ^ hash32);
        }

        /// <summary>
        /// non cryptographic stable hash code,
        /// it will always return the same hash for the same
        /// string.
        /// This is simply an implementation of FNV-1 32 bit
        /// https://en.wikipedia.org/wiki/Fowler%E2%80%93Noll%E2%80%93Vo_hash_function
        /// </summary>
        /// <returns>The stable hash32.</returns>
        /// <param name = "txt">Text.</param>
        public static uint GetStableHashU32(this string txt)
        {
            unchecked
            {
                uint hash = FNV_offset_basis32;
                for (int i = 0; i < txt.Length; i++)
                {
                    uint ch = txt[i];
                    hash = hash * FNV_prime32;
                    hash = hash ^ ch;
                }
                return hash;
            }
        }

        /// <summary>
        /// non cryptographic stable hash code,
        /// it will always return the same hash for the same
        /// string.
        /// This is simply an implementation of FNV-1  64 bit
        /// https://en.wikipedia.org/wiki/Fowler%E2%80%93Noll%E2%80%93Vo_hash_function
        /// </summary>
        /// <returns>The stable hash32.</returns>
        /// <param name = "txt">Text.</param>
        public static ulong GetStableHashU64(this string txt)
        {
            unchecked
            {
                ulong hash = FNV_offset_basis64;
                for (int i = 0; i < txt.Length; i++)
                {
                    ulong ch = txt[i];
                    hash = hash * FNV_prime64;
                    hash = hash ^ ch;
                }
                return hash;
            }
        }
    }
}