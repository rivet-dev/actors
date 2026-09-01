//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

using System;

namespace MonoFN.Cecil.Cil
{
    public enum DocumentType
    {
        Other,
        Text
    }

    public enum DocumentHashAlgorithm
    {
        None,
        MD5,
        SHA1,
        SHA256
    }

    public enum DocumentLanguage
    {
        Other,
        C,
        Cpp,
        CSharp,
        Basic,
        Java,
        Cobol,
        Pascal,
        Cil,
        JScript,
        Smc,
        MCpp,
        FSharp
    }

    public enum DocumentLanguageVendor
    {
        Other,
        Microsoft
    }

    public sealed class Document : DebugInformation
    {
        public string Url { get; set; }
        public DocumentType Type
        {
            get { return TypeGuid.ToType(); }
            set { TypeGuid = value.ToGuid(); }
        }
        public Guid TypeGuid { get; set; }
        public DocumentHashAlgorithm HashAlgorithm
        {
            get { return HashAlgorithmGuid.ToHashAlgorithm(); }
            set { HashAlgorithmGuid = value.ToGuid(); }
        }
        public Guid HashAlgorithmGuid { get; set; }
        public DocumentLanguage Language
        {
            get { return LanguageGuid.ToLanguage(); }
            set { LanguageGuid = value.ToGuid(); }
        }
        public Guid LanguageGuid { get; set; }
        public DocumentLanguageVendor LanguageVendor
        {
            get { return LanguageVendorGuid.ToVendor(); }
            set { LanguageVendorGuid = value.ToGuid(); }
        }
        public Guid LanguageVendorGuid { get; set; }
        public byte[] Hash { get; set; }
        public byte[] EmbeddedSource { get; set; }

        public Document(string url)
        {
            this.Url = url;
            Hash = Empty<byte>.Array;
            EmbeddedSource = Empty<byte>.Array;
            token = new(TokenType.Document);
        }
    }
}