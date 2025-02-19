import React from 'react';
import styled, { createGlobalStyle, keyframes } from 'styled-components';
import Screenshot1 from './path/to/screenshot1.jpg';
import Screenshot2 from './path/to/screenshot2.jpg';
import Screenshot3 from './path/to/screenshot3.jpg';

const fadeIn = keyframes`
  from { opacity: 0; transform: translateY(20px); }
  to { opacity: 1; transform: translateY(0); }
`;

const GlobalStyle = createGlobalStyle`
  :root {
    --bg: linear-gradient(145deg, #0a0a0a 0%, #121212 100%);
    --card-bg: rgba(25, 25, 25, 0.9);
    --text: #e0e0e0;
    --accent: #2b7a78;
    --hover: #3daaaa;
  }

  * {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
  }

  body {
    background: var(--bg);
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
`;

const Container = styled.div`
  max-width: 1200px;
  margin: 0 auto;
`;

const Header = styled.header`
  text-align: center;
  margin-bottom: 3rem;
  animation: ${fadeIn} 0.8s ease;

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

// ... (keep previous styled components, add new ones below)

const FeatureShowcase = styled.section`
  display: grid;
  gap: 2rem;
  margin-top: 3rem;

  @media (min-width: 768px) {
    grid-template-columns: 1fr 1fr;
    align-items: start;
  }
`;

const Screenshot = styled.div`
  border-radius: 12px;
  overflow: hidden;
  border: 1px solid #333;
  transition: transform 0.3s ease;
  
  &:hover {
    transform: translateY(-5px);
  }

  img {
    width: 100%;
    height: auto;
    display: block;
  }

  figcaption {
    padding: 1rem;
    background: var(--card-bg);
    font-size: 0.9rem;
    opacity: 0.8;
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

const App = () => {
  return (
    <>
      <GlobalStyle />
      <Container>
        <Header>
          <h1>Roblox Macro Utilities</h1>
          <p>Windows automation tool that works alongside Roblox - no memory editing, just input automation</p>
          
          <ButtonGroup>
            <Button href="https://github.com/Spencer0187/Roblox-Macro-Utilities/releases/latest" className="primary">
              Download Portable EXE
            </Button>
            <Button href="https://github.com/Spencer0187/Roblox-Macro-Utilities" className="secondary">
              Source Code
            </Button>
          </ButtonGroup>
        </Header>

        <FeatureShowcase>
          <div>
            <h2>Customizable Interface</h2>
            <p>Arrange the tools exactly how you need them:</p>
            <ul>
              <li>Drag buttons to reorganize</li>
              <li>Resize the window freely</li>
              <li>Dark theme matches Roblox Studio</li>
              <li>Settings save automatically</li>
            </ul>
          </div>
          <Screenshot>
            <img src={Screenshot1} alt="Customizable UI with draggable buttons" />
            <figcaption>Drag buttons to reorganize your workspace</figcaption>
          </Screenshot>
        </FeatureShowcase>

        <Section>
          <h2>Complete Feature List</h2>
          <FullFeatureList>
            <div>
              <h3>Core Utilities</h3>
              <ul>
                <li>Persistent anti-AFK</li>
                <li>Window always-on-top toggle</li>
                <li>Portable single EXE file</li>
                <li>Automatic updates checker</li>
              </ul>
            </div>

            <div>
              <h3>Movement Macros</h3>
              <ul>
                <li>Helicopter High Jump</li>
                <li>Speedglitch toggle</li>
                <li>Automatic Ledge Bouncing</li>
                <li>Wallhop Macro</li>
                <li>Wall-Walk assistance</li>
              </ul>
            </div>

            <div>
              <h3>Advanced Actions</h3>
              <ul>
                <li>Dropless Item Desync</li>
                <li>Frame-perfect key presses</li>
                <li>Spam key/button repeater</li>
                <li>Unequip Speedglitch</li>
                <li>Freeze Position Macro</li>
              </ul>
            </div>

            <div>
              <h3>Technical Features</h3>
              <ul>
                <li>Low system resource usage</li>
                <li>Doesn't inject into Roblox</li>
                <li>Regularly maintained</li>
                <li>Open source codebase</li>
              </ul>
            </div>
          </FullFeatureList>
        </Section>

        <FeatureShowcase>
          <Screenshot>
            <img src={Screenshot2} alt="Feature demonstration" />
            <figcaption>Detailed configuration options for each macro</figcaption>
          </Screenshot>
          <div>
            <h2>Precision Control</h2>
            <p>Fine-tune every action with:</p>
            <ul>
              <li>Millisecond timing control</li>
              <li>Keybind customization</li>
              <li>Toggle states visibility</li>
              <li>Visual feedback for active macros</li>
            </ul>
          </div>
        </FeatureShowcase>

        <Section style={{ textAlign: 'center', marginTop: '4rem' }}>
          <h2>Ready to Try It?</h2>
          <ButtonGroup>
            <Button href="https://github.com/Spencer0187/Roblox-Macro-Utilities/releases/latest" className="primary">
              Download Now
            </Button>
            <Button href="https://discord.gg/roblox-glitching-community-998572881892094012" className="secondary">
              Get Support on Discord
            </Button>
          </ButtonGroup>
          <p style={{ marginTop: '1rem', opacity: 0.8 }}>
            Windows 10/11 · 3MB EXE · No installation needed
          </p>
        </Section>

        <Credits>
          <p>Built with:</p>
          <ul>
            <li><a href="https://github.com/ocornut/imgui">ImGui</a> for the interface</li>
            <li><a href="https://github.com/craftwar/suspend">suspend</a> core functionality</li>
          </ul>
        </Credits>
      </Container>
    </>
  );
};
