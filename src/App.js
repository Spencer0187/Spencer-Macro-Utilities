import React, { useState, useEffect } from 'react';
import styled, { createGlobalStyle, keyframes } from 'styled-components';

// ===== Animation Definitions =====
const scrollFade = keyframes`
  from { opacity: 0; transform: translateY(20px); }
  to { opacity: 1; transform: translateY(0); }
`;

const popIn = keyframes`
  from { opacity: 0; transform: scale(0.95); }
  to { opacity: 1; transform: scale(1); }
`;

// ===== Global Styles =====
const GlobalStyle = createGlobalStyle`
  :root {
    --bg-top: #0a0a0a;
    --bg-bottom: #616161;
    --card-bg: rgba(25, 25, 25, 0.9);
    --text: #e0e0e0;
    --accent: #2b7a78;
    --hover: #3daaaa;
    --lightbox-bg: rgba(0, 0, 0, 0.8);
  }

  html {
    scroll-behavior: smooth;
  }

  * {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
  }

  body {
    background: linear-gradient(145deg, var(--bg-top) 0%, var(--bg-bottom) 100%);
    color: var(--text);
    line-height: 1.6;
    font-family: 'Inter', system-ui, sans-serif;
    padding: 2rem 1rem;
    min-height: 100vh;
  }

  a {
    color: var(--hover);
    text-decoration: none;
    transition: opacity 0.2s ease;

    &:hover {
      opacity: 0.8;
      text-decoration: underline;
    }
  }

  ul {
    padding-left: 1.5rem;
    margin: 0.8rem 0;

    li {
      margin-bottom: 0.5rem;
      max-width: 80ch;
      line-height: 1.5;
    }
  }

  @media (max-width: 768px) {
    ul {
      padding-left: 1.2rem;

      li {
        margin-left: 0.5rem;
      }
    }
  }
`;

// ===== Styled Components =====
const Container = styled.div`
  max-width: 1200px;
  margin: 0 auto;
`;

const Header = styled.header`
  text-align: center;
  margin-bottom: 3rem;
  animation: ${popIn} 0.8s ease;

  h1 {
    color: var(--hover);
    font-size: 2.8rem;
    margin-bottom: 1rem;
    letter-spacing: -0.05em;
  }

  p {
    max-width: 600px;
    margin: 0 auto;
    font-size: 1.1rem;
    opacity: 0.9;
  }
`;

const ButtonGroup = styled.div`
  display: flex;
  gap: 1rem;
  justify-content: center;
  margin: 2rem 0;

  @media (max-width: 768px) {
    flex-direction: column;
  }
`;

const Button = styled.a`
  padding: 0.8rem 1.5rem;
  border-radius: 6px;
  text-decoration: none;
  font-weight: 500;
  transition: 0.2s all ease;
  cursor: pointer;

  &.primary {
    background: var(--accent);
    color: white;

    &:hover {
      background: var(--hover);
    }
  }

  &.secondary {
    border: 1px solid var(--accent);
    color: var(--accent);

    &:hover {
      border-color: var(--hover);
      color: var(--hover);
    }
  }
`;

const Section = styled.section`
  background: var(--card-bg);
  border-radius: 8px;
  padding: 2rem;
  margin-bottom: 2rem;
  overflow: hidden;
  animation: ${scrollFade} 0.8s ease;

  h2 {
    color: var(--accent);
    margin-bottom: 1.5rem;
    padding-bottom: 0.5rem;
    border-bottom: 1px solid #333;
  }
`;

const FeatureShowcase = styled.section`
  display: flex;
  flex-direction: column;
  gap: 2rem;
  margin: 4rem 0 2rem;
  animation: ${scrollFade} 0.8s ease;

  @media (min-width: 768px) {
    display: grid;
    grid-template-columns: 1fr 1.5fr;
    align-items: start;
  }

  &:nth-of-type(even) {
    @media (min-width: 768px) {
      grid-template-columns: 1.5fr 1fr;
      direction: rtl;
    }
  }

  &:nth-of-type(even) > * {
    @media (min-width: 768px) {
      direction: ltr;
    }
  }
`;

const Screenshot = styled.div`
  border-radius: 12px;
  overflow: hidden;
  border: 1px solid #333;
  transition: transform 0.3s ease;
  animation: ${popIn} 0.8s ease;
  cursor: pointer;
  position: relative;
  max-height: none;
  width: 100%;
  max-width: 100%;
  box-sizing: border-box;
  padding-right: 1rem;

  @media (min-width: 768px) {
    max-width: none;
    padding-right: 0;
  }

  &:hover {
    transform: translateY(-5px);
  }

  img {
    width: 100%;
    height: auto;
    display: block;
    object-fit: scale-down;
  }

  figcaption {
    padding: 1rem;
    background: var(--card-bg);
    font-size: 0.9rem;
    opacity: 0.8;
    position: absolute;
    bottom: 0;
    left: 0;
    width: 100%;
    box-sizing: border-box;
  }
`;

const FullFeatureList = styled.div`
  columns: 2;
  gap: 2rem;
  margin-top: 2rem;

  @media (max-width: 768px) {
    columns: 1;
  }

  div {
    break-inside: avoid;
    padding: 1rem;
    background: var(--card-bg);
    border-radius: 8px;
    margin-bottom: 1rem;

    h3 {
      color: var(--hover);
      margin-bottom: 0.5rem;
    }

    ul {
      padding-left: 1.2rem;
    }
  }
`;

const Credits = styled.footer`
  opacity: 0.8;
  font-size: 0.9rem;
  margin-top: 2rem;
  padding: 1rem;
  background: rgba(25, 25, 25, 0.9);
  border-radius: 8px;
  animation: ${scrollFade} 0.8s ease;

  a {
    color: var(--hover);
  }
`;

const LightboxOverlay = styled.div`
  position: fixed;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  background: var(--lightbox-bg);
  display: flex;
  justify-content: center;
  align-items: center;
  z-index: 1000;
  cursor: pointer;
`;

const LightboxImage = styled.img`
  max-width: 90%;
  max-height: 90%;
  object-fit: contain;
`;

const DownloadCount = styled.div`
  font-size: 0.9rem;
  opacity: 0.8;
  margin-top: 0.5rem;
  text-align: center;
`;

const CountNumber = styled.span`
  font-weight: bold;
  color: var(--accent);
`;

const FAQSection = styled(Section)`
  text-align: left;

  h3 {
    color: var(--hover);
    margin-bottom: 0.5rem;
  }

  div {
    margin-bottom: 1.5rem;
  }
`;

const KnownIssuesSection = styled(Section)`
  h3 {
    color: var(--accent);
    margin-bottom: 1rem;
  }
`;

// ===== Main Component =====
const App = () => {
  const screenshotUrl1 = 'https://github.com/user-attachments/assets/67332f63-2bb2-4b99-88ad-9169b5148adf';
  const screenshotUrl2 = 'https://github.com/user-attachments/assets/428bc456-dfba-4fe7-8635-e7a2d3deab08';
  const downloadBadgeUrl = 'https://img.shields.io/github/downloads/Spencer0187/Spencer-Macro-Utilities/total.svg';
  const versionBadgeUrl = 'https://img.shields.io/github/v/release/Spencer0187/Spencer-Macro-Utilities';

  const [lightboxImage, setLightboxImage] = useState(null);
  const [downloadCount, setDownloadCount] = useState(null);
  const [loadingCount, setLoadingCount] = useState(true);
  const [countError, setCountError] = useState(null);
  const [currentVersion, setCurrentVersion] = useState(null);
  const [loadingVersion, setLoadingVersion] = useState(true);
  const [versionError, setVersionError] = useState(null);

  useEffect(() => {
    const fetchDownloadCount = async () => {
      setLoadingCount(true);
      setCountError(null);
      try {
        const response = await fetch(downloadBadgeUrl);
        if (!response.ok) {
          throw new Error(`HTTP error! status: ${response.status}`);
        }
        const svgText = await response.text();
        const textMatches = [...svgText.matchAll(/<text[^>]*>(.*?)<\/text>/g)];
        if (textMatches && textMatches.length > 0) {
          const lastText = textMatches[textMatches.length - 1][1];
          setDownloadCount(lastText);
        } else {
          setCountError("Could not extract download count from badge.");
        }
      } catch (error) {
        console.error("Error fetching download count:", error);
        setCountError("Failed to load download count.");
      } finally {
        setLoadingCount(false);
      }
    };
    fetchDownloadCount();
  }, [downloadBadgeUrl]);

  useEffect(() => {
    const fetchVersion = async () => {
      setLoadingVersion(true);
      setVersionError(null);
      try {
        const response = await fetch(versionBadgeUrl);
        if (!response.ok) {
          throw new Error(`HTTP error! status: ${response.status}`);
        }
        const svgText = await response.text();
        const textMatches = [...svgText.matchAll(/<text[^>]*>(.*?)<\/text>/g)];
        if (textMatches && textMatches.length > 0) {
          const lastText = textMatches[textMatches.length - 1][1];
          setCurrentVersion(lastText);
        } else {
          setVersionError("Could not extract version from badge.");
        }
      } catch (error) {
        console.error("Error fetching version:", error);
        setVersionError("Failed to load version.");
      } finally {
        setLoadingVersion(false);
      }
    };
    fetchVersion();
  }, [versionBadgeUrl]);

  const openLightbox = (imageUrl) => {
    setLightboxImage(imageUrl);
  };

  const closeLightbox = () => {
    setLightboxImage(null);
  };

  return (
    <>
      <GlobalStyle />
      <Container>
        <Header>
          <h1>Spencer Macro Utilities</h1>
          <p>
            Windows + Linux automation tool designed for Roblox. <br />
            No memory access, just singular .exe automation.
          </p>

          <ButtonGroup>
            <Button href="https://github.com/Spencer0187/Spencer-Macro-Utilities/releases/latest" className="primary">
              Download (Singular EXE)
            </Button>
            <Button href="https://github.com/Spencer0187/Spencer-Macro-Utilities" className="secondary">
              See Github/Source Code
            </Button>
            <Button href="https://discord.gg/roblox-glitching-community-998572881892094012" className="secondary">
              Join Community Discord
            </Button>
          </ButtonGroup>

          <div style={{ display: 'flex', justifyContent: 'center', gap: '1rem', marginTop: '0.5rem' }}>
            <DownloadCount>
              Total Downloads: {loadingCount ? "Loading..." : countError ? countError : <CountNumber>{downloadCount}</CountNumber>}
            </DownloadCount>
            <DownloadCount>
              Current version: {loadingVersion ? "Loading..." : versionError ? versionError : <CountNumber>{currentVersion}</CountNumber>}
            </DownloadCount>
          </div>
        </Header>

        <FeatureShowcase>
          <div>
            <h2>Theme Editor & Interface</h2>
            <p>Control exactly how the tool looks and feels:</p>
            <ul>
              <li><strong>Fully Fledged Theme Editor:</strong> Customize every color and style</li>
              <li>Drag buttons to any position</li>
              <li>Resize window to your preference</li>
              <li>Layout and settings save automatically</li>
            </ul>
          </div>
          <Screenshot onClick={() => openLightbox(screenshotUrl1)}>
            <img src={screenshotUrl1} alt="Customizable Interface Screenshot" />
            <figcaption>Screenshot of the UI</figcaption>
          </Screenshot>
        </FeatureShowcase>

        <Section>
          <h2>Complete Feature List</h2>
          <FullFeatureList>
            <div>
              <h3>Essential Features</h3>
              <ul>
                <li>Singular .exe file (no extra files)</li>
                <li><strong>Update Prompt:</strong> Optional notification to update on launch</li>
                <li>Persistent anti-AFK</li>
                <li>Low resource usage</li>
              </ul>
            </div>
            <div>
              <h3>Movement Macros</h3>
              <ul>
                <li><strong>New:</strong> Floor Bounce (Itemless/Wall-less High Jump)</li>
                <li>Helicopter High Jump (+Automatic Mode)</li>
                <li>Speedglitch toggle</li>
                <li>Automatic Ledge Bouncing</li>
                <li>Wallhop/Wall-Walk</li>
              </ul>
            </div>
            <div>
              <h3>Advanced Functions</h3>
              <ul>
                <li><strong>WinDivert Lagswitch:</strong> Optimized for Roblox</li>
                <li><strong>Status Overlay:</strong> See lag state while playing</li>
                <li>Item Desync Hitboxes</li>
                <li>Microsecond input timing</li>
                <li>Suspend Processes / Freeze Roblox</li>
              </ul>
            </div>
            <div>
              <h3>Technical Details</h3>
              <ul>
                <li>External input simulation</li>
                <li>No installation required</li>
                <li>Open source C++</li>
                <li>Active maintenance</li>
              </ul>
            </div>
          </FullFeatureList>
        </Section>

        <KnownIssuesSection>
          <h2>Known Issues</h2>
          <ul>
            <li>
              <h3>Application Fails to Launch</h3>
              <p>If the application does not launch, navigate to the file properties and select "Unblock" at the bottom.</p>
            </li>
            <li>
              <h3>Keybinds Not Working</h3>
              <p>If keybinds are not functioning, restart your computer. Ensure you are using the latest version, as updates often contain important fixes.</p>
            </li>
          </ul>
        </KnownIssuesSection>

        <FeatureShowcase>
          <div>
            <h2>Advanced Lagswitch</h2>
            <p>Powerful network manipulation designed specifically for Roblox:</p>
            <ul>
              <li>Full WinDivert integration</li>
              <li><strong>In-Game Overlay:</strong> Enable an overlay to see the lagswitch status while playing</li>
              <li>Customizable duration and settings</li>
              <li>Safety controls to prevent accidental disconnects</li>
            </ul>
          </div>
          <Screenshot onClick={() => openLightbox(screenshotUrl2)}>
            <img src={screenshotUrl2} alt="Precise Control Screenshot" />
            <figcaption>Screenshot of Macro Options</figcaption>
          </Screenshot>
        </FeatureShowcase>

        <Section style={{ textAlign: 'center', marginTop: '3rem' }}>
          <h2>Join the Community Discord</h2>
          <p style={{ maxWidth: '600px', margin: '0.5rem auto 1rem' }}>
            Get notified about updates, discuss glitches, and get help from other users by joining the Roblox Glitching Community Discord server!
          </p>
          <ButtonGroup>
            <Button href="https://discord.gg/roblox-glitching-community-998572881892094012" className="primary">
              Join Discord Server
            </Button>
          </ButtonGroup>
        </Section>

        <FAQSection>
          <h2>FAQ</h2>
          <div>
            <h3>Is this a Cheat?</h3>
            <p>No, this is a macro utility. It simulates user input and does not interact with Roblox's memory or game processes in any way that would be considered cheating. It automates in-game actions through external input.</p>
          </div>
          <div>
            <h3>Windows Defender flags it as a virus!</h3>
            <p>This is a known false positive due to the nature of macro input and WinDivert drivers. If you don't trust the pre-compiled .exe, you can download Visual Studio 2022 with the "Desktop C++" workload and compile the source code yourself from GitHub.</p>
          </div>
          <div>
            <h3>How do I update?</h3>
            <p>The program now includes an optional update notification on launch. Simply click "Yes" when prompted, or download the latest .exe from GitHub.</p>
          </div>
        </FAQSection>

        <Section style={{ textAlign: 'center', marginTop: '4rem' }}>
          <h2>Get Started</h2>
          <ButtonGroup>
            <Button href="https://github.com/Spencer0187/Spencer-Macro-Utilities/releases/latest" className="primary">
              Download (Singular EXE)
            </Button>
            <Button href="https://discord.gg/roblox-glitching-community-998572881892094012" className="secondary">
              Join Community
            </Button>
          </ButtonGroup>
          <div style={{ display: 'flex', justifyContent: 'center', gap: '1rem', marginTop: '1rem' }}>
            <DownloadCount>
              Total Downloads: {loadingCount ? "Loading..." : countError ? countError : <CountNumber>{downloadCount}</CountNumber>}
            </DownloadCount>
            <DownloadCount>
              Current version: {loadingVersion ? "Loading..." : versionError ? versionError : <CountNumber>{currentVersion}</CountNumber>}
            </DownloadCount>
          </div>
          <p style={{ marginTop: '1rem', opacity: 0.8 }}>
            Windows 10/11 / Linux · Portable EXE · No dependencies
          </p>
          <p style={{ marginTop: '1rem', opacity: 0.7, fontSize: '0.9rem' }}>
            Discord server features update pings and glitch discussion
          </p>
        </Section>

        <Credits>
          <p>Implementation Details:</p>
          <ul>
            <li><a href="https://github.com/ocornut/imgui">ImGui</a> interface framework</li>
            <li><a href="https://github.com/basil00/WinDivert">WinDivert</a> network manipulation</li>
            <li><a href="https://github.com/craftwar/suspend">Suspend</a> process handling</li>
          </ul>
        </Credits>
      </Container>

      {lightboxImage && (
        <LightboxOverlay onClick={closeLightbox}>
          <LightboxImage src={lightboxImage} alt="Lightbox View" />
        </LightboxOverlay>
      )}
    </>
  );
};

export default App;
