//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

using MonoFN.Cecil.PE;
using MonoFN.Security.Cryptography;
using System;
using System.IO;
using System.Reflection;
using System.Runtime.Serialization;
using System.Security.Cryptography;

namespace MonoFN.Cecil
{
    // Most of this code has been adapted
    // from Jeroen Frijters' fantastic work
    // in IKVM.Reflection.Emit. Thanks!

    internal static class CryptoService
    {
        public static byte[] GetPublicKey(WriterParameters parameters)
        {
            using (RSA rsa = parameters.CreateRSA())
            {
                byte[] cspBlob = CryptoConvert.ToCapiPublicKeyBlob(rsa);
                byte[] publicKey = new byte [12 + cspBlob.Length];
                Buffer.BlockCopy(cspBlob, 0, publicKey, 12, cspBlob.Length);
                // The first 12 bytes are documented at:
                // http://msdn.microsoft.com/library/en-us/cprefadd/html/grfungethashfromfile.asp
                // ALG_ID - Signature
                publicKey[1] = 36;
                // ALG_ID - Hash
                publicKey[4] = 4;
                publicKey[5] = 128;
                // Length of Public Key (in bytes)
                publicKey[8] = (byte)(cspBlob.Length >> 0);
                publicKey[9] = (byte)(cspBlob.Length >> 8);
                publicKey[10] = (byte)(cspBlob.Length >> 16);
                publicKey[11] = (byte)(cspBlob.Length >> 24);
                return publicKey;
            }
        }

        public static void StrongName(Stream stream, ImageWriter writer, WriterParameters parameters)
        {
            int strong_name_pointer;

            byte[] strong_name = CreateStrongName(parameters, HashStream(stream, writer, out strong_name_pointer));
            PatchStrongName(stream, strong_name_pointer, strong_name);
        }

        private static void PatchStrongName(Stream stream, int strong_name_pointer, byte[] strong_name)
        {
            stream.Seek(strong_name_pointer, SeekOrigin.Begin);
            stream.Write(strong_name, 0, strong_name.Length);
        }

        private static byte[] CreateStrongName(WriterParameters parameters, byte[] hash)
        {
            const string hash_algo = "SHA1";

            using (RSA rsa = parameters.CreateRSA())
            {
                RSAPKCS1SignatureFormatter formatter = new(rsa);
                formatter.SetHashAlgorithm(hash_algo);

                byte[] signature = formatter.CreateSignature(hash);
                Array.Reverse(signature);

                return signature;
            }
        }

        private static byte[] HashStream(Stream stream, ImageWriter writer, out int strong_name_pointer)
        {
            const int buffer_size = 8192;

            Section text = writer.text;
            int header_size = (int)writer.GetHeaderSize();
            int text_section_pointer = (int)text.PointerToRawData;
            DataDirectory strong_name_directory = writer.GetStrongNameSignatureDirectory();

            if (strong_name_directory.Size == 0)
                throw new InvalidOperationException();

            strong_name_pointer = (int)(text_section_pointer + (strong_name_directory.VirtualAddress - text.VirtualAddress));
            int strong_name_length = (int)strong_name_directory.Size;

            SHA1Managed sha1 = new();
            byte[] buffer = new byte [buffer_size];
            using (CryptoStream crypto_stream = new(Stream.Null, sha1, CryptoStreamMode.Write))
            {
                stream.Seek(0, SeekOrigin.Begin);
                CopyStreamChunk(stream, crypto_stream, buffer, header_size);

                stream.Seek(text_section_pointer, SeekOrigin.Begin);
                CopyStreamChunk(stream, crypto_stream, buffer, (int)strong_name_pointer - text_section_pointer);

                stream.Seek(strong_name_length, SeekOrigin.Current);
                CopyStreamChunk(stream, crypto_stream, buffer, (int)(stream.Length - (strong_name_pointer + strong_name_length)));
            }

            return sha1.Hash;
        }

        private static void CopyStreamChunk(Stream stream, Stream dest_stream, byte[] buffer, int length)
        {
            while (length > 0)
            {
                int read = stream.Read(buffer, 0, Math.Min(buffer.Length, length));
                dest_stream.Write(buffer, 0, read);
                length -= read;
            }
        }

        public static byte[] ComputeHash(string file)
        {
            if (!File.Exists(file))
                return Empty<byte>.Array;

            using (FileStream stream = new(file, FileMode.Open, FileAccess.Read, FileShare.Read))
            {
                return ComputeHash(stream);
            }
        }

        public static byte[] ComputeHash(Stream stream)
        {
            const int buffer_size = 8192;

            SHA1Managed sha1 = new();
            byte[] buffer = new byte [buffer_size];

            using (CryptoStream crypto_stream = new(Stream.Null, sha1, CryptoStreamMode.Write))
            {
                CopyStreamChunk(stream, crypto_stream, buffer, (int)stream.Length);
            }

            return sha1.Hash;
        }

        public static byte[] ComputeHash(params ByteBuffer[] buffers)
        {
            SHA1Managed sha1 = new();

            using (CryptoStream crypto_stream = new(Stream.Null, sha1, CryptoStreamMode.Write))
            {
                for (int i = 0; i < buffers.Length; i++)
                {
                    crypto_stream.Write(buffers[i].buffer, 0, buffers[i].length);
                }
            }

            return sha1.Hash;
        }

        public static Guid ComputeGuid(byte[] hash)
        {
            // From corefx/src/System.Reflection.Metadata/src/System/Reflection/Metadata/BlobContentId.cs
            byte[] guid = new byte [16];
            Buffer.BlockCopy(hash, 0, guid, 0, 16);

            // modify the guid data so it decodes to the form of a "random" guid ala rfc4122
            guid[7] = (byte)((guid[7] & 0x0f) | (4 << 4));
            guid[8] = (byte)((guid[8] & 0x3f) | (2 << 6));

            return new(guid);
        }
    }

    internal static partial class Mixin
    {
        public static RSA CreateRSA(this WriterParameters writer_parameters)
        {
            byte[] key;
            string key_container;

            if (writer_parameters.StrongNameKeyBlob != null)
                return CryptoConvert.FromCapiKeyBlob(writer_parameters.StrongNameKeyBlob);

            if (writer_parameters.StrongNameKeyContainer != null)
                key_container = writer_parameters.StrongNameKeyContainer;
            else if (!TryGetKeyContainer(writer_parameters.StrongNameKeyPair, out key, out key_container))
                return CryptoConvert.FromCapiKeyBlob(key);

            CspParameters parameters = new()
            {
                Flags = CspProviderFlags.UseMachineKeyStore,
                KeyContainerName = key_container,
                KeyNumber = 2
            };

            return new RSACryptoServiceProvider(parameters);
        }

        private static bool TryGetKeyContainer(ISerializable key_pair, out byte[] key, out string key_container)
        {
            SerializationInfo info = new(typeof(StrongNameKeyPair), new FormatterConverter());
            key_pair.GetObjectData(info, new());

            key = (byte[])info.GetValue("_keyPairArray", typeof(byte[]));
            key_container = info.GetString("_keyPairContainer");
            return key_container != null;
        }
    }
}